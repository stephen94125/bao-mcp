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
    net::IpAddr,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::{Mutex as AsyncMutex, Semaphore};

const SHARDS: usize = 64;
const MAX_TRACKED_CALLERS: usize = 1024;
const VERIFIED_MAX_AGE: Duration = Duration::from_secs(300);
const NEW_AUTH_PER_SECOND: usize = 24;
const KNOWN_AUTH_PER_SECOND: usize = 8;
const SOURCE_AUTH_PER_SECOND: usize = 4;
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
    shards: [[AsyncMutex<()>; SHARDS]; 2],
    admission: Mutex<Admission>,
    new_auth: Semaphore,
    known_auth: Semaphore,
    global_attempts: Mutex<VecDeque<Instant>>,
    concurrent: Semaphore,
}
// This state only allocates admission capacity; it never authorizes a request.
// Keep hashes, not raw tokens, and never evict live verified callers for new ones.
#[derive(Default)]
struct Admission {
    verified: BTreeMap<String, Verified>,
    unknown_attempts: BTreeMap<String, Instant>,
    sources: BTreeMap<Option<IpAddr>, VecDeque<Instant>>,
    new_attempts: VecDeque<Instant>,
    known_attempts: VecDeque<Instant>,
}
struct Verified {
    until: Instant,
    last_attempt: Option<Instant>,
}
fn prune_rate(attempts: &mut VecDeque<Instant>, time: Instant) {
    while attempts
        .front()
        .is_some_and(|t| time.saturating_duration_since(*t) >= Duration::from_secs(1))
    {
        attempts.pop_front();
    }
}
impl Admission {
    fn prune(&mut self, time: Instant) {
        self.verified.retain(|_, value| value.until > time);
        self.unknown_attempts
            .retain(|_, t| time.saturating_duration_since(*t) < REFRESH_MIN);
        self.sources.retain(|_, attempts| {
            prune_rate(attempts, time);
            !attempts.is_empty()
        });
        prune_rate(&mut self.new_attempts, time);
        prune_rate(&mut self.known_attempts, time);
    }
    fn known(&mut self, hash: &str, time: Instant) -> bool {
        self.prune(time);
        self.verified.contains_key(hash)
    }
    fn reserve(&mut self, hash: &str, peer: Option<IpAddr>, time: Instant) -> Result<bool> {
        self.prune(time);
        if let Some(caller) = self.verified.get_mut(hash) {
            if caller
                .last_attempt
                .is_some_and(|t| time.saturating_duration_since(t) < REFRESH_MIN)
                || self.known_attempts.len() >= KNOWN_AUTH_PER_SECOND
            {
                return Err(throttled());
            }
            caller.last_attempt = Some(time);
            self.known_attempts.push_back(time);
            return Ok(true);
        }
        if self.unknown_attempts.contains_key(hash)
            || self.unknown_attempts.len() >= MAX_TRACKED_CALLERS
            || self.new_attempts.len() >= NEW_AUTH_PER_SECOND
            || self
                .sources
                .get(&peer)
                .is_some_and(|v| v.len() >= SOURCE_AUTH_PER_SECOND)
            || (!self.sources.contains_key(&peer) && self.sources.len() >= MAX_TRACKED_CALLERS)
        {
            return Err(throttled());
        }
        self.unknown_attempts.insert(hash.into(), time);
        self.sources.entry(peer).or_default().push_back(time);
        self.new_attempts.push_back(time);
        Ok(false)
    }
    fn remember(&mut self, hash: &str, ttl: u64, time: Instant) {
        self.prune(time);
        if self.verified.contains_key(hash) || self.verified.len() < MAX_TRACKED_CALLERS {
            let lifetime = if ttl == 0 {
                VERIFIED_MAX_AGE
            } else {
                VERIFIED_MAX_AGE.min(Duration::from_secs(ttl))
            };
            self.verified.insert(
                hash.into(),
                Verified {
                    until: time + lifetime,
                    last_attempt: Some(time),
                },
            );
        }
    }
    fn invalidate(&mut self, hash: &str) {
        self.unknown_attempts.remove(hash);
        if let Some(caller) = self.verified.get_mut(hash) {
            caller.last_attempt = None;
        }
    }
}

impl Discovery {
    pub fn new(backend: Backend, cache: Cache) -> Self {
        Self(Arc::new(Inner {
            backend,
            cache,
            shards: std::array::from_fn(|_| std::array::from_fn(|_| AsyncMutex::new(()))),
            admission: Mutex::new(Admission::default()),
            new_auth: Semaphore::new(6),
            known_auth: Semaphore::new(2),
            global_attempts: Mutex::new(VecDeque::new()),
            concurrent: Semaphore::new(8),
        }))
    }

    pub async fn get(&self, credential: &Credential, refresh: bool) -> Result<Snapshot> {
        self.get_from_peer(credential, refresh, None).await
    }

    /// Only pass the transport peer, never caller-supplied forwarding headers.
    /// Without peer information, all unknown callers share a conservative bucket.
    pub async fn get_from_peer(
        &self,
        credential: &Credential,
        refresh: bool,
        peer: Option<IpAddr>,
    ) -> Result<Snapshot> {
        let scope = self.0.backend.scope_id();
        let hash = credential.hash();
        if !refresh {
            if let Some(snapshot) = self.0.cache.load(&scope, &hash).await? {
                return Ok(snapshot);
            }
        }
        let known = self
            .0
            .admission
            .lock()
            .map_err(|_| Error::internal())?
            .known(&hash, Instant::now());
        // Bound waiters before they can hold HTTP slots on a shard lock.
        // In particular, duplicate unknown tokens must not form an unbounded
        // queue ahead of authentication admission.
        let auth = if known {
            &self.0.known_auth
        } else {
            &self.0.new_auth
        };
        let auth_permit = auth.try_acquire().map_err(|_| throttled())?;
        // Unknown token traffic cannot hold a verified caller's shard lock.
        let shard = &self.0.shards[usize::from(known)][shard_index(&hash)];
        let _lock = shard.lock().await;
        if !refresh {
            if let Some(snapshot) = self.0.cache.load(&scope, &hash).await? {
                return Ok(snapshot);
            }
        }
        self.0
            .admission
            .lock()
            .map_err(|_| Error::internal())?
            .reserve(&hash, peer, Instant::now())?;
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
                    self.0
                        .admission
                        .lock()
                        .map_err(|_| Error::internal())?
                        .verified
                        .remove(&hash);
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
        self.0
            .admission
            .lock()
            .map_err(|_| Error::internal())?
            .remember(&hash, ttl, Instant::now());
        drop(auth_permit);
        // Invalid tokens never consume the expensive metadata discovery quota.
        self.reserve_fetch(Instant::now())?;
        let _permit = self.0.concurrent.try_acquire().map_err(|_| throttled())?;
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
        self.0
            .admission
            .lock()
            .map_err(|_| Error::internal())?
            .invalidate(&hash);
        Ok(())
    }

    fn reserve_fetch(&self, instant: Instant) -> Result<()> {
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
    fn authentication_lanes_are_bounded_and_independent() {
        let mut admission = Admission::default();
        let time = Instant::now();
        for i in 0..NEW_AUTH_PER_SECOND {
            let peer = Some(IpAddr::from([192, 0, 2, i as u8]));
            assert!(
                !admission
                    .reserve(&format!("unknown-{i}"), peer, time)
                    .unwrap()
            );
        }
        assert_eq!(
            admission
                .reserve("overflow", None, time)
                .unwrap_err()
                .status,
            429
        );
        for i in 0..KNOWN_AUTH_PER_SECOND {
            let hash = format!("known-{i}");
            admission.remember(&hash, 3600, time - REFRESH_MIN);
            assert!(admission.reserve(&hash, None, time).unwrap());
        }
        admission.remember("known-overflow", 3600, time - REFRESH_MIN);
        assert!(admission.reserve("known-overflow", None, time).is_err());
        assert!(
            admission
                .reserve(
                    "unknown-0",
                    Some(IpAddr::from([192, 0, 2, 0])),
                    time + REFRESH_MIN
                )
                .is_ok()
        );
    }

    #[test]
    fn source_and_token_limits_do_not_evict_live_records() {
        let mut admission = Admission::default();
        let time = Instant::now();
        for i in 0..SOURCE_AUTH_PER_SECOND {
            admission
                .reserve(&format!("token-{i}"), None, time)
                .unwrap();
        }
        assert!(admission.reserve("fifth-token", None, time).is_err());
        assert!(
            admission
                .reserve("token-0", Some(IpAddr::from([192, 0, 2, 1])), time)
                .is_err()
        );
        assert!(
            admission
                .reserve("other-source", Some(IpAddr::from([192, 0, 2, 1])), time)
                .is_ok()
        );
        for i in 0..MAX_TRACKED_CALLERS {
            admission.remember(&format!("verified-{i}"), 3600, time);
        }
        admission.remember("untracked", 3600, time);
        assert_eq!(admission.verified.len(), MAX_TRACKED_CALLERS);
        assert!(!admission.known("untracked", time));
        assert!(admission.known("verified-0", time));
        admission.invalidate("verified-0");
        assert!(admission.reserve("verified-0", None, time).unwrap());
        assert!(!admission.known("verified-0", time + VERIFIED_MAX_AGE));
        admission.remember("short", 1, time + VERIFIED_MAX_AGE);
        assert!(!admission.known("short", time + VERIFIED_MAX_AGE + Duration::from_secs(1)));
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
