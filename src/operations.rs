//! Stateless transformations from grouped tools to native OpenBao requests.
use crate::{
    Error, Result,
    backend::{Backend, Credential},
    catalog,
};
use serde_json::{Map, Value, json};

const OPERATIONS: &[&str] = &[
    "read", "write", "create", "update", "patch", "delete", "list", "scan",
];

fn invalid(message: impl AsRef<str>) -> Error {
    Error::invalid(message.as_ref())
}

fn validate_args(name: &str, args: Value) -> Result<Map<String, Value>> {
    let definition = catalog::get(name).ok_or_else(|| invalid("Unknown tool"))?;
    let mut args = args
        .as_object()
        .cloned()
        .ok_or_else(|| invalid("Tool arguments must be an object"))?;
    let schema = definition.input_schema;
    let properties = schema["properties"]
        .as_object()
        .expect("catalog properties");
    for key in args.keys() {
        if !properties.contains_key(key) {
            return Err(invalid(format!("Unknown argument: {key}")));
        }
    }
    for key in schema["required"].as_array().into_iter().flatten() {
        let key = key.as_str().expect("catalog required field");
        if !args.contains_key(key) {
            return Err(invalid(format!("Missing required argument: {key}")));
        }
    }
    for (key, property) in properties {
        if !args.contains_key(key) {
            if let Some(default) = property.get("default") {
                args.insert(key.clone(), default.clone());
            }
        }
        if let Some(value) = args.get(key) {
            let valid = match property["type"].as_str() {
                Some("string") => value.is_string(),
                Some("object") => value.is_object(),
                Some("boolean") => value.is_boolean(),
                Some("integer") => value.is_i64() || value.is_u64(),
                _ => false,
            };
            if !valid {
                return Err(invalid(format!("Invalid argument type: {key}")));
            }
            if let Some(allowed) = property["enum"].as_array() {
                if !allowed.contains(value) {
                    return Err(invalid(format!("Unsupported value for {key}")));
                }
            }
        }
    }
    Ok(args)
}

fn string<'a>(args: &'a Map<String, Value>, key: &str) -> Result<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| invalid(format!("Missing string argument: {key}")))
}

/// Keep routing unambiguous and prevent URL/query/header injection. Names and
/// native key paths are otherwise Bao's concern, including Unicode and colons.
fn relative_path(path: &str, allow_empty: bool) -> Result<()> {
    if path.is_empty() {
        return if allow_empty {
            Ok(())
        } else {
            Err(invalid("path must not be empty"))
        };
    }
    if path.len() > 4096
        || path.starts_with('/')
        || path.contains(['?', '#', '\\', '%'])
        || path.chars().any(char::is_control)
        || path
            .split('/')
            .any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        return Err(invalid(
            "path must be a relative native API path without URL, query, fragment, encoding or dot segments",
        ));
    }
    Ok(())
}

fn mount_segment(mount: &str) -> Result<()> {
    if mount.is_empty()
        || mount.len() > 128
        || !mount.as_bytes()[0].is_ascii_alphanumeric() && mount.as_bytes()[0] != b'_'
        || !mount
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
    {
        return Err(invalid(
            "mount must be a single safe mount segment; use bao_request for nested mounts",
        ));
    }
    Ok(())
}

fn query(args: &Map<String, Value>) -> Result<Vec<(String, String)>> {
    let Some(value) = args.get("query") else {
        return Ok(Vec::new());
    };
    let mut output = Vec::new();
    for (key, value) in value.as_object().expect("validated query") {
        if key.is_empty() || key.len() > 256 || key.chars().any(char::is_control) {
            return Err(invalid(
                "query keys must be nonempty native parameter names",
            ));
        }
        let value = match value {
            Value::String(value) => value.clone(),
            Value::Bool(value) => value.to_string(),
            Value::Number(value) => value.to_string(),
            _ => return Err(invalid("query values must be strings, numbers or booleans")),
        };
        output.push((key.clone(), value));
    }
    Ok(output)
}

fn wrap_ttl(args: &Map<String, Value>) -> Result<Option<&str>> {
    let Some(value) = args.get("wrap_ttl") else {
        return Ok(None);
    };
    let value = value.as_str().expect("validated wrap_ttl");
    if value.is_empty()
        || value.len() > 64
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.')
    {
        return Err(invalid(
            "wrap_ttl must be a native duration, for example 60s",
        ));
    }
    Ok(Some(value))
}

fn has_head(path: &str, heads: &[&str]) -> bool {
    heads.iter().any(|head| {
        path == *head
            || path
                .strip_prefix(*head)
                .is_some_and(|tail| tail.starts_with('/'))
    })
}

fn scoped_path(name: &str, path: &str) -> Result<String> {
    let allowed = match name {
        "bao_request" | "identity" | "token" | "system" => true,
        "kv_manage" => has_head(
            path,
            &[
                "metadata", "delete", "undelete", "destroy", "config", "subkeys",
            ],
        ),
        "ssh_ca" => has_head(path, &["config", "issuer", "issuers", "public_key"]),
        "ssh_roles" => has_head(path, &["roles"]),
        "ssh_issue" => has_head(
            path,
            &["sign", "issue", "creds", "verify", "lookup", "tidy"],
        ),
        "pki_ca" => has_head(
            path,
            &[
                "root",
                "intermediate",
                "issuer",
                "issuers",
                "key",
                "keys",
                "ca",
                "ca_chain",
            ],
        ),
        "pki_roles" => has_head(path, &["roles", "cel/roles"]),
        "pki_certificates" => has_head(
            path,
            &[
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
            ],
        ),
        "pki_config" => has_head(path, &["config", "eab"]),
        "transit_keys" => has_head(
            path,
            &[
                "keys",
                "export",
                "byok-export",
                "backup",
                "restore",
                "wrapping_key",
                "config/keys",
                "cache-config",
            ],
        ),
        "transit_crypto" => has_head(
            path,
            &[
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
            ],
        ),
        "database_connections" => has_head(path, &["config", "reset", "rotate-root"]),
        "database_roles" => has_head(path, &["roles", "static-roles"]),
        "database_credentials" => has_head(path, &["creds", "static-creds", "rotate-role"]),
        "kubernetes" => has_head(path, &["config", "check", "roles", "creds"]),
        "ldap" => has_head(
            path,
            &[
                "config",
                "rotate-root",
                "static-role",
                "static-cred",
                "rotate-role",
                "role",
                "creds",
                "library",
            ],
        ),
        "rabbitmq" => has_head(
            path,
            &["config/connection", "config/lease", "roles", "creds"],
        ),
        "totp_keys" => has_head(path, &["keys"]),
        "totp_code" => has_head(path, &["code"]),
        "auth" => has_head(path, &["auth", "sys/auth"]),
        "policy" => has_head(path, &["policies", "policy"]),
        "mounts" => has_head(path, &["mounts", "remount"]),
        "audit" => has_head(
            path,
            &["audit", "audit-hash", "config/auditing/request-headers"],
        ),
        "cubbyhole" | "leases" | "namespaces" => true,
        _ => false,
    };
    if !allowed {
        return Err(invalid(
            "path is outside this tool family; use bao_request for other native endpoints",
        ));
    }
    let prefix = match name {
        "cubbyhole" => "cubbyhole",
        "identity" => "identity",
        "token" => "auth/token",
        "policy" | "mounts" | "audit" | "system" => "sys",
        "leases" => "sys/leases",
        "namespaces" => "sys/namespaces",
        _ => "",
    };
    Ok(if prefix.is_empty() {
        path.into()
    } else if path.is_empty() {
        prefix.into()
    } else {
        format!("{prefix}/{path}")
    })
}

fn is_mounted(name: &str) -> bool {
    matches!(
        catalog::family(name),
        Some(
            "kv" | "ssh"
                | "pki"
                | "transit"
                | "database"
                | "kubernetes"
                | "ldap"
                | "rabbitmq"
                | "totp"
        )
    )
}

pub async fn execute(
    backend: &Backend,
    credential: &Credential,
    name: &str,
    args: Value,
) -> Result<Value> {
    let args = validate_args(name, args)?;
    if name == "vault_create" {
        return vault_create(backend, credential, &args).await;
    }
    if name == "api_help" {
        let path = args.get("path").and_then(Value::as_str).unwrap_or("");
        relative_path(path, true)?;
        if path.is_empty() {
            let generic = args["generic_mount_paths"]
                .as_bool()
                .unwrap_or(false)
                .to_string();
            return backend
                .request(
                    credential,
                    "read",
                    "sys/internal/specs/openapi",
                    None,
                    &[("generic_mount_paths".into(), generic)],
                    None,
                )
                .await;
        }
        if args.get("generic_mount_paths").and_then(Value::as_bool) == Some(true) {
            return Err(invalid(
                "generic_mount_paths is only valid when path is omitted",
            ));
        }
        return backend
            .request(
                credential,
                "read",
                path,
                None,
                &[("help".into(), "1".into())],
                None,
            )
            .await;
    }
    if name == "cache_refresh" {
        return Err(invalid("cache_refresh is handled by the MCP server"));
    }

    let operation = string(&args, "operation")?;
    if !OPERATIONS.contains(&operation) {
        return Err(invalid("Unknown native operation"));
    }
    let path = string(&args, "path")?;
    let allow_empty = matches!(name, "kv_read" | "cubbyhole" | "leases" | "namespaces");
    relative_path(path, allow_empty)?;
    if matches!(name, "kv_read" | "cubbyhole")
        && path.is_empty()
        && !matches!(operation, "list" | "scan")
    {
        return Err(invalid("An empty key path is only valid for list or scan"));
    }
    let mut query = query(&args)?;
    let wrap_ttl = wrap_ttl(&args)?;
    let body = args.get("data").cloned();

    let native_path = if matches!(name, "kv_read" | "kv_write") {
        let mount = string(&args, "mount")?;
        mount_segment(mount)?;
        let engine_version = args["engine_version"]
            .as_u64()
            .expect("validated engine_version");
        if let Some(version) = args.get("version") {
            let version = version
                .as_u64()
                .filter(|version| *version > 0)
                .ok_or_else(|| invalid("version must be a positive integer"))?;
            if engine_version != 2 || operation != "read" {
                return Err(invalid("version is only valid for KV v2 reads"));
            }
            if query.iter().any(|(key, _)| key == "version") {
                return Err(invalid("Specify version once, using version or query"));
            }
            query.push(("version".into(), version.to_string()));
        }
        if operation == "patch" && engine_version != 2 {
            return Err(invalid("KV patch requires engine_version 2"));
        }
        if name == "kv_write"
            && engine_version == 2
            && !body
                .as_ref()
                .and_then(|body| body.get("data"))
                .is_some_and(Value::is_object)
        {
            return Err(invalid(
                "KV v2 data must be the native envelope containing an object data field",
            ));
        }
        let suffix = if engine_version == 1 {
            path.to_owned()
        } else if matches!(operation, "list" | "scan") {
            if path.is_empty() {
                "metadata".into()
            } else {
                format!("metadata/{path}")
            }
        } else {
            format!("data/{path}")
        };
        if suffix.is_empty() {
            mount.to_owned()
        } else {
            format!("{mount}/{suffix}")
        }
    } else {
        let path = scoped_path(name, path)?;
        if is_mounted(name) {
            let mount = string(&args, "mount")?;
            mount_segment(mount)?;
            format!("{mount}/{path}")
        } else {
            path
        }
    };
    backend
        .request(credential, operation, &native_path, body, &query, wrap_ttl)
        .await
}

/// Some OpenBao versions return 400 for an absent individual mount. Only the
/// authenticated native mount catalog can turn that ambiguous result into an
/// absence; a denied or malformed catalog must never authorize an enable.
async fn probe_mount(
    backend: &Backend,
    credential: &Credential,
    mount: &str,
) -> Result<Option<Value>> {
    let path = format!("sys/mounts/{mount}");
    match backend
        .request(credential, "read", &path, None, &[], None)
        .await
    {
        Ok(response) => Ok(Some(response.get("data").unwrap_or(&response).clone())),
        Err(error) if error.status == 404 => Ok(None),
        Err(error) if error.status == 400 => {
            let response = backend
                .request(credential, "read", "sys/mounts", None, &[], None)
                .await?;
            let invalid_catalog = || {
                Error::new(
                    502,
                    "invalid_mount_catalog",
                    "OpenBao did not return a usable native mount catalog",
                )
            };
            let mounts = response
                .get("data")
                .and_then(Value::as_object)
                .ok_or_else(invalid_catalog)?;
            if mounts.iter().any(|(path, metadata)| {
                !path.ends_with('/')
                    || path.len() <= 1
                    || !metadata.is_object()
                    || metadata.get("type").and_then(Value::as_str).is_none()
            }) {
                return Err(invalid_catalog());
            }
            Ok(mounts.get(&format!("{mount}/")).cloned())
        }
        Err(error) => Err(error),
    }
}

/// Allocation is intentionally sequential and nontransactional. Each completed
/// mount is visible in the result, including when a later native request fails.
async fn vault_create(
    backend: &Backend,
    credential: &Credential,
    args: &Map<String, Value>,
) -> Result<Value> {
    let vault = string(args, "vault_id")?;
    if vault.is_empty()
        || vault.len() > 64
        || !vault.as_bytes()[0].is_ascii_alphanumeric()
        || !vault
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
    {
        return Err(invalid(
            "vault_id must be 1-64 ASCII alphanumeric, underscore or dash characters and start alphanumeric",
        ));
    }
    let description = args
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or("");
    let mounts = [
        (vault.to_owned(), "kv"),
        (format!("ssh-ca-{vault}"), "ssh"),
        ("_sys_pki_root".into(), "pki"),
        ("_sys_pki_int".into(), "pki"),
    ];
    let mut results = Vec::new();
    let mut failure: Option<Error> = None;
    for (mount, kind) in &mounts {
        let mut entry = json!({"mount":mount,"type":kind,"shared":mount.starts_with("_sys_pki_")});
        if failure.is_some() {
            entry["status"] = json!("not_attempted");
            results.push(entry);
            continue;
        }
        let path = format!("sys/mounts/{mount}");
        let existing = probe_mount(backend, credential, mount).await;
        match existing {
            Ok(Some(data)) => {
                if data.get("type").and_then(Value::as_str) != Some(*kind)
                    || (*kind == "kv" && !matches!(data["options"]["version"].as_str(), Some("2")))
                {
                    let error = Error::new(
                        409,
                        "mount_conflict",
                        "An existing bundle mount has an incompatible engine type or KV version",
                    );
                    entry["status"] = json!("conflict");
                    entry["error"] = json!(error);
                    failure = Some(error);
                } else {
                    entry["status"] = json!("existing");
                }
            }
            Ok(None) => {
                let mut body = json!({"type":kind});
                if *kind == "kv" {
                    body["options"] = json!({"version":"2"});
                }
                if !description.is_empty() {
                    body["description"] = json!(description);
                }
                match backend
                    .request(credential, "write", &path, Some(body), &[], None)
                    .await
                {
                    Ok(_) => entry["status"] = json!("created"),
                    Err(error) => {
                        // A transport failure may follow a successful enable;
                        // never claim absence or retry that mutation silently.
                        entry["status"] = json!("failed");
                        entry["outcome"] = json!("unconfirmed; inspect the mount before retrying");
                        entry["error"] = json!(error);
                        failure = Some(error);
                    }
                }
            }
            Err(error) => {
                entry["status"] = json!("failed");
                entry["error"] = json!(error);
                failure = Some(error);
            }
        }
        results.push(entry);
    }
    let complete = failure.is_none();
    let changed = results.iter().any(|entry| entry["status"] == "created");
    Ok(json!({
        "vault_id":vault,
        "complete":complete,
        "status":if complete { "complete" } else if changed { "partial" } else { "failed" },
        "allocation":{"kv":vault,"ssh":format!("ssh-ca-{vault}"),"pki_root":"_sys_pki_root","pki_intermediate":"_sys_pki_int","pki_scope":"shared within the configured namespace"},
        "mounts":results,
        "error":failure,
        "ca_initialization_performed":false,
        "note":"Only native mounts are allocated. Existing settings are retained; configure CA keys, issuers and roles explicitly. No rollback is performed."
    }))
}
