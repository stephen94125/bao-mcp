//! Approximate tool discovery; OpenBao remains the authorizer for every operation.

use crate::{
    Error, Result,
    backend::{Backend, Credential},
    cache::{Cache, now},
    catalog,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::{Mutex as AsyncMutex, Semaphore};

const SHARDS: usize = 64;
const MAX_ATTEMPTS_PER_SHARD: usize = 16;
const REFRESH_MIN: Duration = Duration::from_secs(5);
const FETCHES_PER_SECOND: usize = 32;
const MAX_PATHS: usize = 1024;
const MAX_MOUNTS: usize = 512;
const PATH_BATCH: usize = 64;

/// Only allowlisted, nonsensitive discovery metadata is serializable into cache.db.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Snapshot {
    pub tools: Vec<String>,
    pub capabilities: Value,
    pub mounts: Value,
    pub complete: bool,
    pub warnings: Vec<String>,
    pub fetched_at: u64,
    pub expires_at: u64,
}

#[derive(Clone)]
pub struct Discovery(Arc<Inner>);
struct Inner {
    backend: Backend,
    cache: Cache,
    shards: [Shard; SHARDS],
    global_attempts: Mutex<VecDeque<Instant>>,
    concurrent: Semaphore,
}
struct Shard {
    lock: AsyncMutex<()>,
    attempts: Mutex<VecDeque<(String, Instant)>>,
}

impl Discovery {
    pub fn new(backend: Backend, cache: Cache) -> Self {
        Self(Arc::new(Inner {
            backend,
            cache,
            shards: std::array::from_fn(|_| Shard {
                lock: AsyncMutex::new(()),
                attempts: Mutex::new(VecDeque::new()),
            }),
            global_attempts: Mutex::new(VecDeque::new()),
            concurrent: Semaphore::new(8),
        }))
    }

    pub async fn get(&self, credential: &Credential, refresh: bool) -> Result<Snapshot> {
        let scope = self.0.backend.scope_id();
        let hash = credential.hash();
        if !refresh {
            if let Some(snapshot) = self.0.cache.load(&scope, &hash).await? {
                return Ok(snapshot);
            }
        }
        let shard = &self.0.shards[shard_index(&hash)];
        let _lock = shard.lock.lock().await;
        if !refresh {
            if let Some(snapshot) = self.0.cache.load(&scope, &hash).await? {
                return Ok(snapshot);
            }
        }
        // All cold/refresh attempts consume the same bounded rate budget, including unknown tokens.
        self.reserve_attempt(shard, &hash, Instant::now())?;
        let _permit = self.0.concurrent.try_acquire().map_err(|_| throttled())?;
        let fetched_at = now();
        // Never persist this raw response: data.id contains the caller's token.
        let identity = match self
            .0
            .backend
            .request(
                credential,
                "read",
                "auth/token/lookup-self",
                None,
                &[],
                None,
            )
            .await
        {
            Ok(value) => value,
            Err(error) => {
                if matches!(error.status, 401 | 403) {
                    self.0.cache.invalidate(&scope, &hash).await?;
                }
                return Err(error);
            }
        };
        let ttl = identity
            .get("data")
            .and_then(|data| data.get("ttl"))
            .and_then(Value::as_u64)
            .ok_or_else(|| {
                Error::new(
                    502,
                    "discovery_invalid_response",
                    "OpenBao token metadata did not include a usable TTL",
                )
            })?;
        drop(identity);
        let expires_at = self.0.cache.fresh_until(fetched_at).min(if ttl == 0 {
            u64::MAX
        } else {
            fetched_at.saturating_add(ttl)
        });
        let snapshot = match self.fetch(credential, fetched_at, expires_at).await {
            Ok(snapshot) => snapshot,
            Err(error) => {
                if error.status == 401 {
                    self.0.cache.invalidate(&scope, &hash).await?;
                }
                return Err(error);
            }
        };
        if snapshot.expires_at <= now() {
            return Err(Error::unauthorized());
        }
        self.0.cache.store(&scope, &hash, &snapshot).await?;
        Ok(snapshot)
    }

    pub async fn invalidate(&self, credential: &Credential) -> Result<()> {
        let hash = credential.hash();
        self.0
            .cache
            .invalidate(&self.0.backend.scope_id(), &hash)
            .await?;
        // A native permission failure invalidates discovery, but must not lock
        // a valid caller out of its other paths for the refresh cooldown.
        // Revalidation still consumes the bounded process-wide fetch budget.
        self.0.shards[shard_index(&hash)]
            .attempts
            .lock()
            .map_err(|_| Error::internal())?
            .retain(|(key, _)| key != &hash);
        Ok(())
    }

    fn reserve_attempt(&self, shard: &Shard, hash: &str, instant: Instant) -> Result<()> {
        let mut attempts = shard.attempts.lock().map_err(|_| Error::internal())?;
        attempts.retain(|(_, time)| instant.saturating_duration_since(*time) < REFRESH_MIN);
        if attempts.iter().any(|(key, _)| key == hash) {
            return Err(throttled());
        }
        let mut global = self
            .0
            .global_attempts
            .lock()
            .map_err(|_| Error::internal())?;
        while global
            .front()
            .is_some_and(|time| instant.saturating_duration_since(*time) >= Duration::from_secs(1))
        {
            global.pop_front();
        }
        if global.len() >= FETCHES_PER_SECOND {
            return Err(throttled());
        }
        global.push_back(instant);
        // Do not evict recent per-key limits: reject a full shard instead.
        if attempts.len() >= MAX_ATTEMPTS_PER_SHARD {
            return Err(throttled());
        }
        attempts.push_back((hash.to_owned(), instant));
        Ok(())
    }

    async fn metadata(
        &self,
        credential: &Credential,
        path: &str,
        warnings: &mut Vec<String>,
    ) -> Result<Option<Value>> {
        match self
            .0
            .backend
            .request(credential, "read", path, None, &[], None)
            .await
        {
            Ok(value) => Ok(Some(value)),
            Err(error) if matches!(error.status, 403 | 404) => {
                warnings.push(format!("OpenBao did not expose discovery metadata at {path}; related tool visibility may be incomplete"));
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }

    async fn fetch(
        &self,
        credential: &Credential,
        fetched_at: u64,
        expires_at: u64,
    ) -> Result<Snapshot> {
        let mut warnings = vec!["Tool visibility is approximate. OpenBao 2.7.2 resultant ACL omits '+' wildcard rules; capabilities only describe the concrete paths queried, and grouped tools do not grant every argument. Use bao_request for undiscovered paths; OpenBao authorizes every request.".into()];
        let mount_response = self
            .metadata(credential, "sys/internal/ui/mounts", &mut warnings)
            .await?;
        let acl_response = self
            .metadata(credential, "sys/internal/ui/resultant-acl", &mut warnings)
            .await?;
        let mut mounts = BTreeMap::new();
        if let Some(data) = mount_response.as_ref().and_then(|v| v.get("data")) {
            for table in ["secret", "auth"] {
                if let Some(entries) = data.get(table).and_then(Value::as_object) {
                    for (path, value) in entries {
                        if mounts.len() >= MAX_MOUNTS {
                            warnings
                                .push("Mount metadata exceeded the bounded discovery limit".into());
                            break;
                        }
                        let path = if table == "auth" {
                            format!("auth/{path}")
                        } else {
                            path.to_owned()
                        };
                        if !valid_path(&path) {
                            continue;
                        }
                        let Some(kind) = value
                            .get("type")
                            .and_then(Value::as_str)
                            .filter(|s| s.len() <= 128)
                        else {
                            continue;
                        };
                        // options may contain arbitrary configuration: retain only the KV version.
                        let version = value
                            .pointer("/options/version")
                            .and_then(Value::as_str)
                            .filter(|v| matches!(*v, "1" | "2"));
                        mounts.insert(path, json!({"type":kind,"version":version}));
                    }
                }
            }
        }
        drop(mount_response);
        let root = acl_response
            .as_ref()
            .and_then(|v| v.pointer("/data/root"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let mut candidates: BTreeSet<String> = management_candidates()
            .into_iter()
            .map(str::to_owned)
            .collect();
        let mut acl_paths = Map::new();
        if let Some(data) = acl_response.as_ref().and_then(|v| v.get("data")) {
            for table in ["exact_paths", "glob_paths"] {
                if let Some(paths) = data.get(table).and_then(Value::as_object) {
                    for (path, value) in paths {
                        let Some(path) = relative_acl_path(path, self.0.backend.namespace()) else {
                            continue;
                        };
                        if !valid_path(&path) || acl_paths.len() >= MAX_PATHS {
                            continue;
                        }
                        let caps = capabilities(value.get("capabilities"));
                        // Do not retain ACL parameter values; they can contain sensitive configuration.
                        let key = if table == "glob_paths" {
                            format!("{path}*")
                        } else {
                            path.clone()
                        };
                        acl_paths.insert(key, json!(caps));
                        if table == "exact_paths" {
                            candidates.insert(path);
                        } else {
                            candidates.insert(format!("{}__bao_discovery_probe__", path));
                        }
                    }
                }
            }
        }
        drop(acl_response);
        for (path, metadata) in &mounts {
            candidates.insert(path.trim_end_matches('/').to_owned());
            for suffix in engine_candidates(
                metadata.get("type").and_then(Value::as_str).unwrap_or(""),
                metadata.get("version").and_then(Value::as_str),
            ) {
                candidates.insert(format!(
                    "{}{suffix}",
                    if path.ends_with('/') {
                        path.to_owned()
                    } else {
                        format!("{path}/")
                    }
                ));
            }
        }
        if candidates.len() > MAX_PATHS {
            warnings.push("Candidate paths exceeded the bounded discovery limit".into());
        }
        let paths: Vec<String> = candidates
            .into_iter()
            .filter(|p| valid_path(p))
            .take(MAX_PATHS)
            .collect();
        let mut confirmed = Map::new();
        for batch in paths.chunks(PATH_BATCH) {
            match self
                .0
                .backend
                .request(
                    credential,
                    "write",
                    "sys/capabilities-self",
                    Some(json!({"paths":batch})),
                    &[],
                    None,
                )
                .await
            {
                Ok(value) => {
                    let Some(data) = value.get("data").and_then(Value::as_object) else {
                        warnings.push("OpenBao returned no usable path capability map".into());
                        continue;
                    };
                    for path in batch {
                        let caps = data.get(path).or_else(|| {
                            if batch.len() == 1 {
                                data.get("capabilities")
                            } else {
                                None
                            }
                        });
                        if caps.is_some() {
                            confirmed.insert(path.clone(), json!(capabilities(caps)));
                        }
                    }
                }
                Err(error) if matches!(error.status, 403 | 404) => {
                    warnings.push("OpenBao denied concrete capability discovery; tool visibility may be incomplete".into());
                    break;
                }
                Err(error) => return Err(error),
            }
        }
        if !mounts.is_empty() {
            warnings.push("Visible mounts are metadata hints, not proof of path permission. Tools without a confirmed matching path can be omitted; bao_request remains available for '+' wildcard grants and other undiscovered paths".into());
        }
        let tools = catalog::all()
            .into_iter()
            .filter(|definition| {
                root || matches!(catalog::family(&definition.name), Some("generic" | "cache"))
                    || confirmed
                        .iter()
                        .any(|(path, caps)| tool_matches(&definition.name, path, caps, &mounts))
            })
            .map(|definition| definition.name.to_string())
            .collect();
        Ok(Snapshot {
            tools,
            capabilities: json!({"root":root,"paths":confirmed,"acl_patterns":acl_paths,"authoritative_for_execution":false}),
            mounts: json!(mounts),
            complete: false,
            warnings,
            fetched_at,
            expires_at,
        })
    }
}

fn throttled() -> Error {
    Error::new(
        429,
        "discovery_rate_limited",
        "Discovery refresh is rate limited; retry after at least five seconds",
    )
}
fn shard_index(hash: &str) -> usize {
    usize::from_str_radix(&hash[..2], 16).unwrap_or(0) % SHARDS
}
fn capabilities(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter(|cap| {
            matches!(
                *cap,
                "create"
                    | "read"
                    | "update"
                    | "delete"
                    | "list"
                    | "scan"
                    | "patch"
                    | "sudo"
                    | "root"
                    | "deny"
            )
        })
        .map(str::to_owned)
        .collect()
}
fn valid_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 1024
        && !path.starts_with('/')
        && !path
            .chars()
            .any(|c| c.is_control() || matches!(c, '\\' | '%' | '?' | '#' | '*' | '+'))
        && !path
            .trim_end_matches('/')
            .split('/')
            .any(|s| matches!(s, "" | "." | ".."))
}
fn relative_acl_path(path: &str, namespace: &str) -> Option<String> {
    if namespace.is_empty() {
        return Some(path.to_owned());
    }
    path.strip_prefix(&format!("{}/", namespace.trim_matches('/')))
        .map(str::to_owned)
}
fn mount_family<'a>(path: &str, value: &'a Value) -> &'a str {
    if path.starts_with("auth/") {
        return if path.starts_with("auth/token/") {
            "token"
        } else {
            "auth"
        };
    }
    match value.get("type").and_then(Value::as_str).unwrap_or("") {
        "kv" | "generic" => "kv",
        "ssh" => "ssh",
        "pki" => "pki",
        "transit" => "transit",
        "database" => "database",
        "totp" => "totp",
        "kubernetes" => "kubernetes",
        "ldap" | "openldap" => "ldap",
        "rabbitmq" => "rabbitmq",
        "cubbyhole" | "ns_cubbyhole" => "cubbyhole",
        "identity" | "ns_identity" => "identity",
        "system" | "ns_system" => "system",
        _ => "generic",
    }
}
fn tool_matches(name: &str, path: &str, caps: &Value, mounts: &BTreeMap<String, Value>) -> bool {
    let capabilities = capabilities(Some(caps));
    if capabilities.iter().any(|cap| cap == "deny") {
        return false;
    }
    let has = |verbs: &[&str]| {
        capabilities
            .iter()
            .any(|cap| cap == "root" || verbs.contains(&cap.as_str()))
    };
    let any = has(&[
        "read", "list", "scan", "create", "update", "patch", "delete",
    ]);
    if !any {
        return false;
    }
    if name == "vault_create" {
        return path.starts_with("sys/mounts/") && has(&["create", "update"]);
    }
    let Some(family) = catalog::family(name) else {
        return false;
    };
    if path_family(path, mounts) != family {
        return false;
    }
    let mount = mounts
        .iter()
        .filter(|(prefix, _)| {
            path.starts_with(prefix.as_str()) || path == prefix.trim_end_matches('/')
        })
        .max_by_key(|(prefix, _)| prefix.len());
    let relative = mount
        .map(|(prefix, _)| path.strip_prefix(prefix).unwrap_or(""))
        .unwrap_or(path);
    let starts = |prefixes: &[&str]| {
        prefixes
            .iter()
            .any(|prefix| relative == *prefix || relative.starts_with(&format!("{prefix}/")))
    };
    match name {
        "kv_read" => {
            let v2 = mount.is_some_and(|(_, value)| {
                value.get("version").and_then(Value::as_str) == Some("2")
            });
            if v2 {
                (starts(&["data"]) && has(&["read"]))
                    || (starts(&["metadata"]) && has(&["list", "scan"]))
            } else {
                has(&["read", "list", "scan"])
            }
        }
        "kv_write" => {
            let v2 = mount.is_some_and(|(_, value)| {
                value.get("version").and_then(Value::as_str) == Some("2")
            });
            (!v2 || starts(&["data"])) && has(&["create", "update", "patch"])
        }
        "kv_manage" => starts(&[
            "metadata", "delete", "undelete", "destroy", "config", "subkeys",
        ]),
        "ssh_ca" => starts(&["config", "issuer", "issuers", "public_key"]),
        "ssh_roles" => starts(&["roles"]),
        "ssh_issue" => starts(&["sign", "issue", "creds", "verify", "lookup", "tidy"]),
        "pki_ca" => starts(&[
            "root",
            "intermediate",
            "issuer",
            "issuers",
            "key",
            "keys",
            "ca",
            "ca_chain",
        ]),
        "pki_roles" => starts(&["roles", "cel/roles"]),
        "pki_certificates" => starts(&[
            "issue",
            "sign",
            "sign-verbatim",
            "cert",
            "certs",
            "revoke",
            "revoke-with-key",
            "crl",
            "unified-crl",
            "unified-certs",
            "ocsp",
            "unified-ocsp",
            "tidy",
            "tidy-cancel",
            "tidy-status",
            "cel/issue",
            "cel/sign",
        ]),
        "pki_config" => starts(&["config", "eab"]),
        "transit_keys" => starts(&[
            "keys",
            "export",
            "byok-export",
            "backup",
            "restore",
            "wrapping_key",
            "config/keys",
            "cache-config",
        ]),
        "transit_crypto" => starts(&[
            "encrypt",
            "decrypt",
            "sign",
            "verify",
            "rewrap",
            "hmac",
            "hash",
            "random",
            "datakey",
            "derive-key",
        ]),
        "database_connections" => starts(&["config", "reset", "rotate-root"]),
        "database_roles" => starts(&["roles", "static-roles"]),
        "database_credentials" => starts(&["creds", "static-creds", "rotate-role"]),
        "totp_keys" => starts(&["keys"]),
        "totp_code" => starts(&["code"]),
        _ => true,
    }
}
fn path_family<'a>(path: &str, mounts: &'a BTreeMap<String, Value>) -> &'a str {
    for (prefix, family) in [
        ("auth/token/", "token"),
        ("sys/policies/", "policy"),
        ("sys/policy", "policy"),
        ("sys/leases/", "leases"),
        ("sys/mounts", "mounts"),
        ("sys/namespaces", "namespaces"),
        ("sys/audit", "audit"),
        ("sys/auth", "auth"),
        ("identity/", "identity"),
    ] {
        if path == prefix.trim_end_matches('/') || path.starts_with(prefix) {
            return family;
        }
    }
    if let Some((mount, value)) = mounts
        .iter()
        .filter(|(mount, _)| {
            path.starts_with(mount.as_str()) || path == mount.trim_end_matches('/')
        })
        .max_by_key(|(mount, _)| mount.len())
    {
        return mount_family(mount, value);
    }
    if path.starts_with("sys/") {
        "system"
    } else if path.starts_with("auth/") {
        "auth"
    } else {
        "generic"
    }
}
fn management_candidates() -> Vec<&'static str> {
    vec![
        "sys/mounts",
        "sys/mounts/__bao_discovery_probe__",
        "sys/auth",
        "sys/policies/acl",
        "sys/leases/lookup",
        "sys/namespaces",
        "sys/audit",
        "sys/health",
        "sys/seal-status",
        "sys/config/state/sanitized",
        "auth/token/create",
        "auth/token/roles",
        "identity/entity",
        "identity/group",
        "cubbyhole/__bao_discovery_probe__",
    ]
}
fn engine_candidates(kind: &str, version: Option<&str>) -> Vec<&'static str> {
    match kind {
        "kv" | "generic" if version == Some("2") => vec![
            "data/__bao_discovery_probe__",
            "metadata/__bao_discovery_probe__",
            "config",
        ],
        "kv" | "generic" | "cubbyhole" | "ns_cubbyhole" => vec!["__bao_discovery_probe__"],
        "ssh" => vec![
            "roles",
            "config/ca",
            "sign/__bao_discovery_probe__",
            "creds/__bao_discovery_probe__",
        ],
        "pki" => vec![
            "roles",
            "cert/ca",
            "config/urls",
            "issuers",
            "issue/__bao_discovery_probe__",
        ],
        "transit" => vec![
            "keys",
            "encrypt/__bao_discovery_probe__",
            "decrypt/__bao_discovery_probe__",
        ],
        "database" => vec!["roles", "config", "creds/__bao_discovery_probe__"],
        "kubernetes" => vec!["config", "roles", "creds/__bao_discovery_probe__"],
        "ldap" | "openldap" => vec![
            "config",
            "role",
            "roles",
            "creds/__bao_discovery_probe__",
            "static-role",
            "static-cred/__bao_discovery_probe__",
        ],
        "rabbitmq" => vec![
            "config/connection",
            "roles",
            "creds/__bao_discovery_probe__",
        ],
        "totp" => vec!["keys", "code/__bao_discovery_probe__"],
        _ => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn namespace_prefix_is_exact_and_metadata_is_allowlisted() {
        assert_eq!(
            relative_acl_path("agent/kv/data/a", "agent").as_deref(),
            Some("kv/data/a")
        );
        assert!(relative_acl_path("agent-other/kv/a", "agent").is_none());
        assert_eq!(
            capabilities(Some(&json!(["read", "secret-value", "deny"]))),
            vec!["read", "deny"]
        );
        assert!(!valid_path("secret/+/credentials"));
    }
    #[test]
    fn refresh_attempts_are_bounded_and_include_unknown_tokens() {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::open(
            &dir.path().join("cache.db"),
            Duration::from_secs(10),
            Duration::from_secs(10),
            2,
        )
        .unwrap();
        let backend =
            Backend::new("http://127.0.0.1:1", "agent", None, Duration::from_secs(1)).unwrap();
        let discovery = Discovery::new(backend, cache);
        let instant = Instant::now();
        let shard = &discovery.0.shards[0];
        discovery
            .reserve_attempt(shard, "invalid-token-hash", instant)
            .unwrap();
        assert_eq!(
            discovery
                .reserve_attempt(shard, "invalid-token-hash", instant)
                .unwrap_err()
                .status,
            429
        );
        discovery
            .reserve_attempt(shard, "invalid-token-hash", instant + REFRESH_MIN)
            .unwrap();
        for i in 0..MAX_ATTEMPTS_PER_SHARD - 1 {
            discovery
                .reserve_attempt(shard, &i.to_string(), instant + REFRESH_MIN)
                .unwrap();
        }
        assert!(
            discovery
                .reserve_attempt(shard, "overflow", instant + REFRESH_MIN)
                .is_err()
        );
        assert_eq!(shard.attempts.lock().unwrap().len(), MAX_ATTEMPTS_PER_SHARD);
    }
}

#[cfg(test)]
mod namespace_builtin_tests {
    use super::*;
    #[test]
    fn native_namespace_builtin_types_are_discovered() {
        let mounts = BTreeMap::from([
            ("sys/".into(), json!({"type":"ns_system"})),
            ("cubbyhole/".into(), json!({"type":"ns_cubbyhole"})),
            ("identity/".into(), json!({"type":"ns_identity"})),
        ]);
        assert!(tool_matches(
            "system",
            "sys/health",
            &json!(["read"]),
            &mounts
        ));
        assert!(tool_matches(
            "cubbyhole",
            "cubbyhole/key",
            &json!(["read"]),
            &mounts
        ));
        assert!(tool_matches(
            "identity",
            "identity/entity",
            &json!(["read"]),
            &mounts
        ));
    }
}
