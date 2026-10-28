//! A small, native-API-oriented catalog. OpenBao remains the authority for every
//! operation; these families organize its paths without narrowing native bodies.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Definition {
    pub name: String,
    pub description: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
}

fn text(description: &str) -> Value {
    json!({"type":"string","description":description})
}

fn request_schema(mounted: bool, path_description: &str) -> Value {
    let mut schema = json!({
        "type":"object",
        "additionalProperties":false,
        "properties":{
            "operation":{
                "type":"string",
                "enum":["read","write","create","update","patch","delete","list","scan"],
                "description":"Native OpenBao operation: read=GET, write/create/update=PUT, patch=PATCH, delete=DELETE; list/scan use Bao's native query selectors. Bao evaluates the actual path and caller token."
            },
            "path":text(path_description),
            "data":{
                "type":"object",
                "additionalProperties":true,
                "description":"Complete native JSON request body, forwarded unchanged. Supply all endpoint fields here; the gateway does not whitelist engine fields. Omit for bodyless requests."
            },
            "query":{
                "type":"object",
                "additionalProperties":{"type":["string","number","boolean"]},
                "description":"Native query parameters as scalar values, for example {\"version\":2}. Authentication and namespace overrides are not supported."
            },
            "wrap_ttl":text("Optional native response wrapping TTL, for example 60s. The wrapping token is returned directly to the caller and is never cached.")
        },
        "required":["operation","path"]
    });
    if mounted {
        schema["properties"]["mount"] = json!({
            "type":"string", "pattern":"^[A-Za-z0-9_-][A-Za-z0-9_.-]{0,127}$",
            "description":"One engine mount segment, without /v1 or slashes, for example secret, ssh-ca-team or _sys_pki_int. For nested mounts use bao_request."
        });
        schema["required"]
            .as_array_mut()
            .unwrap()
            .push(json!("mount"));
    }
    schema
}

fn tool(name: &str, description: &str, schema: Value) -> Definition {
    Definition {
        name: name.into(),
        description: description.into(),
        input_schema: schema,
    }
}

fn grouped(name: &str, description: &str, mounted: bool, paths: &str) -> Definition {
    tool(name, description, request_schema(mounted, paths))
}

pub fn all() -> Vec<Definition> {
    let mut kv_read = request_schema(
        true,
        "Secret key relative to the mount, for example apps/api. Use an empty path only for a root list/scan. KV v2 routes reads to data/ and lists/scans to metadata/.",
    );
    kv_read["properties"]["operation"] =
        json!({"type":"string","enum":["read","list","scan"],"default":"read"});
    kv_read["properties"]["engine_version"] = json!({"type":"integer","enum":[1,2],"default":2,"description":"KV engine version; the gateway does not guess it."});
    kv_read["properties"]["version"] = json!({"type":"integer","minimum":1,"description":"Historical KV v2 version, valid only with operation read."});
    kv_read["properties"]
        .as_object_mut()
        .unwrap()
        .remove("data");
    kv_read["required"] = json!(["mount", "path"]);

    let mut kv_write = request_schema(
        true,
        "Secret key relative to the mount, for example apps/api. The data/ prefix is inserted for KV v2.",
    );
    kv_write["properties"]["operation"] =
        json!({"type":"string","enum":["write","create","update","patch"],"default":"write"});
    kv_write["properties"]["engine_version"] = json!({"type":"integer","enum":[1,2],"default":2});
    kv_write["properties"]["data"]["description"] = json!(
        "Complete native body: KV v2 {\"data\":{\"password\":\"...\"},\"options\":{\"cas\":0}}; KV v1 is the values object itself. PATCH uses the same native KV v2 envelope. Unknown engine fields are preserved."
    );
    kv_write["required"] = json!(["mount", "path", "data"]);

    vec![
        grouped(
            "bao_request",
            "Native OpenBao JSON API escape hatch, including plugins and advanced endpoints. Use a /v1-relative path and a native operation; the JSON object data is forwarded unchanged. Examples: read path=auth/token/lookup-self; write path=sys/wrapping/unwrap data={}. All requests use the current caller token and the configured namespace. This route supports nested mounts and JSON endpoints outside grouped families. Arbitrary HTTP headers, raw binary uploads and the ACME client protocol are outside this interface; native ACME configuration is available through pki_config.",
            false,
            "Full native API path without /v1, URL, query string, fragment, or namespace prefix. Example: transit/encrypt/customer-key.",
        ),
        tool(
            "api_help",
            "Discover an authorized native endpoint with GET ?help=1. Example path=transit/encrypt/customer-key returns its native fields and supported operations. Omit path to request sys/internal/specs/openapi for mounted API discovery; OpenBao may require additional permissions. Discovery does not grant execution rights.",
            json!({
                "type":"object","additionalProperties":false,
                "properties":{
                    "path":text("Optional /v1-relative endpoint; omit for sys/internal/specs/openapi."),
                    "generic_mount_paths":{"type":"boolean","default":false,"description":"Only for full OpenAPI discovery: ask Bao to use generic mount paths."}
                }
            }),
        ),
        tool(
            "cache_refresh",
            "Discard and refresh only the current caller's derived tool discovery metadata. Secret values, issued credentials, tokens, certificates and cryptographic responses are never cached. OpenBao is queried with the caller token; refresh does not change Bao policy or engine configuration.",
            json!({"type":"object","properties":{},"additionalProperties":false}),
        ),
        tool(
            "vault_create",
            "Allocate the native engine bundle for a vault: KV v2 at <vault_id>, SSH at ssh-ca-<vault_id>, and shared PKI mounts _sys_pki_root / _sys_pki_int in the configured namespace. Only missing mounts are enabled. Existing compatible mounts are retained; mismatched types or KV versions stop the operation. No namespace, owner metadata, CA, key or role is generated. Results identify each created/existing/failed/not-attempted mount; earlier allocations remain on a later failure, so retry is safe after inspecting the result.",
            json!({
                "type":"object","additionalProperties":false,
                "properties":{
                    "vault_id":{"type":"string","pattern":"^[A-Za-z0-9][A-Za-z0-9_-]{0,63}$","description":"Vault identifier used literally for the KV and SSH mount names."},
                    "description":text("Optional description for newly enabled mounts. Existing descriptions and configuration are not rewritten.")
                },"required":["vault_id"]
            }),
        ),
        tool(
            "kv_read",
            "Read historical/current KV values or list/scan keys using explicit KV engine_version (default 2). Example mount=team path=apps/api version=2; list uses operation=list path=apps. Native envelopes, lease metadata and warnings are preserved in responses. No owner metadata or local secret schema is required.",
            kv_read,
        ),
        tool(
            "kv_write",
            "Write or merge-patch KV values with the complete native request body. KV v2 example mount=team path=apps/api data={\"data\":{\"password\":\"...\"},\"options\":{\"cas\":0}}. Set operation=patch for JSON merge patch; CAS and all native fields are preserved. KV v1 uses engine_version=1 and a plain values body.",
            kv_write,
        ),
        grouped(
            "kv_manage",
            "Manage KV v2 metadata, version deletion/undeletion/destruction, mount configuration and subkeys. Paths are native mount-relative paths: metadata/apps/api, delete/apps/api, undelete/apps/api, destroy/apps/api, config, subkeys/apps/api. Example write path=destroy/apps/api data={\"versions\":[1,2]}; patch path=metadata/apps/api data={\"custom_metadata\":{\"owner\":\"team\"}}. Deleting metadata permanently removes all versions; Bao enforces its own permissions.",
            true,
            "Native KV v2 path beginning metadata, delete, undelete, destroy, config, or subkeys; includes its key suffix.",
        ),
        grouped(
            "ssh_ca",
            "Manage SSH CA keys, issuer import/selection and CA configuration. Example write path=config/ca data={\"generate_signing_key\":true}; read path=public_key; list path=issuers; write path=issuers/import data={...}. CA generation and replacement occur only when explicitly requested here.",
            true,
            "Native SSH path: config/ca, config/issuers, config/zeroaddress, issuer/<ref>, issuers/import or public_key.",
        ),
        grouped(
            "ssh_roles",
            "Read, list, create, update or delete native SSH roles. Example write path=roles/server data={\"key_type\":\"ca\",\"allowed_users\":\"ubuntu\",\"allow_user_certificates\":true}; list path=roles. Forward any additional native role fields in data.",
            true,
            "roles or roles/<role>.",
        ),
        grouped(
            "ssh_issue",
            "Sign caller SSH public keys, issue keypairs, generate legacy SSH credentials, verify or look up OTPs, and tidy dynamic keys. Example write path=sign/server data={\"public_key\":\"ssh-ed25519 ...\",\"valid_principals\":\"ubuntu\"}; write path=issue/server data={...}. Responses may contain private keys; they are returned only to the caller.",
            true,
            "sign/<role>, issue/<role>, creds/<role>, verify, lookup, or tidy/dynamic-keys.",
        ),
        grouped(
            "pki_ca",
            "Manage native PKI roots, intermediates, issuers and keys, including generation/import/rotation, cross-signing and issuer-specific signing. Examples write path=root/generate/internal data={\"common_name\":\"Example CA\"}; write path=intermediate/set-signed data={\"certificate\":\"...\"}; list path=issuers; patch path=issuer/default data={...}. Key and CA generation are explicit operations; no automatic setup occurs.",
            true,
            "root[/...], intermediate/..., issuer/<ref>[/...], issuers[/...], key/<ref>[/...], keys[/...], ca[/pem], or ca_chain.",
        ),
        grouped(
            "pki_roles",
            "Manage PKI issuance roles and CEL roles using complete native bodies. Example write path=roles/web data={\"allowed_domains\":[\"example.com\"],\"allow_subdomains\":true}; list path=roles; write path=cel/roles/web data={...}. Role constraints and issued identity are decided by Bao.",
            true,
            "roles[/<name>] or cel/roles[/<name>].",
        ),
        grouped(
            "pki_certificates",
            "Issue/sign/revoke certificates and manage certificate inventory, CRLs, OCSP and tidy operations. Examples write path=issue/web data={\"common_name\":\"app.example.com\"}; write path=sign/web data={\"csr\":\"...\"}; write path=revoke data={\"serial_number\":\"...\"}; read path=cert/<serial>; write path=crl/rotate. For issuer-specific signing use pki_ca path=issuer/<ref>/sign/<role>. Raw native responses remain subject to backend content limits.",
            true,
            "issue/..., sign/..., sign-verbatim[/...], cert/..., certs[/...], revoke, revoke-with-key, crl[/...], unified-crl[/...], unified-certs[/...], ocsp[/...], unified-ocsp[/...], tidy, tidy-cancel, tidy-status, or cel/issue|sign/... .",
        ),
        grouped(
            "pki_config",
            "Configure native PKI URLs, CRL, cluster, issuers, keys, auto-tidy and ACME, and manage ACME EAB records. Example write path=config/acme data={\"enabled\":true,\"allowed_roles\":[\"web\"]}; write path=config/urls data={...}; list path=eab. The ACME client protocol is outside this interface; this tool manages its native configuration.",
            true,
            "config/<name>[/...] or eab[/<key_id>].",
        ),
        grouped(
            "transit_keys",
            "Manage Transit keys and versions, rotation/configuration, import/import_version, export/BYOK export, backup/restore, certificates and wrapping keys. Examples write path=keys/customer data={\"type\":\"aes256-gcm96\"}; write path=keys/customer/rotate; read path=export/encryption-key/customer/1; read path=backup/customer; write path=restore/customer data={\"backup\":\"...\"}. Export requires Bao key configuration and ACLs; no key material is cached.",
            true,
            "keys[/...], export/..., byok-export/..., backup/..., restore[/...], wrapping_key, config/keys, or cache-config.",
        ),
        grouped(
            "transit_crypto",
            "Perform Transit encryption/decryption, signing/verification, rewrap, HMAC, hashing, random bytes, data-key and key-derivation operations. Example write path=encrypt/customer data={\"plaintext\":\"base64...\"}; write path=sign/signing-key/sha2-256 data={\"input\":\"base64...\"}; write path=random/32. Preserve native batch_input, context, key_version and algorithm fields in data.",
            true,
            "encrypt/..., decrypt/..., sign/..., verify/..., rewrap/..., hmac/..., hash[/algorithm], random[/source][/bytes], datakey/..., or derive-key/... .",
        ),
        grouped(
            "database_connections",
            "Configure/reset native database connections and rotate root credentials. Example write path=config/postgres data={\"plugin_name\":\"postgresql-database-plugin\",\"connection_url\":\"...\",\"allowed_roles\":[\"app\"]}; write path=rotate-root/postgres. Plugin-specific connection fields are passed through unchanged.",
            true,
            "config[/<connection>], reset/<connection>, or rotate-root/<connection>.",
        ),
        grouped(
            "database_roles",
            "Manage dynamic and static database roles. Example write path=roles/app data={\"db_name\":\"postgres\",\"creation_statements\":[\"...\"],\"default_ttl\":\"1h\"}; write path=static-roles/service data={...}; list path=roles. Bao owns rotation settings and role state.",
            true,
            "roles[/<role>] or static-roles[/<role>].",
        ),
        grouped(
            "database_credentials",
            "Generate dynamic database credentials, read static credentials, or rotate a static role. Examples read path=creds/app; read path=static-creds/service; write path=rotate-role/service. Credentials and leases are returned directly and never cached.",
            true,
            "creds/<role>, static-creds/<role>, or rotate-role/<role>.",
        ),
        grouped(
            "kubernetes",
            "Manage a native Kubernetes secrets engine: configuration, configuration checks, roles and service-account credential generation. Example write path=roles/app data={\"allowed_kubernetes_namespaces\":[\"apps\"],\"service_account_name\":\"app\"}; write path=creds/app data={\"kubernetes_namespace\":\"apps\",\"ttl\":\"1h\"}; read path=check. All Kubernetes role, token and generated-object options pass through unchanged. Kubernetes authentication configuration belongs to auth path=auth/<mount>/... .",
            true,
            "config, check, roles[/<role>], or creds/<role>, relative to the Kubernetes secrets mount.",
        ),
        grouped(
            "ldap",
            "Manage an installed and enabled native-compatible LDAP secrets plugin: configuration/root rotation, static and dynamic roles/credentials, and service-account library checkout/check-in. LDAP is distributed as an external plugin in OpenBao 2.7; this tool does not install it. Examples read path=static-cred/app; write path=rotate-role/app; write path=library/team/check-out data={\"ttl\":\"1h\"}; write path=role/app data={...}. Native schema, LDIF and password options pass through unchanged. LDAP authentication uses the separate auth tool.",
            true,
            "config, rotate-root, static-role[/<role>], static-cred/<role>, rotate-role/<role>, role[/<role>], creds/<role>, or library[/<set>[/status|check-out|check-in]], including library/manage/<set>/check-in.",
        ),
        grouped(
            "rabbitmq",
            "Manage a native RabbitMQ secrets engine: connection settings, lease settings, role permissions and dynamic credentials. Examples write path=config/connection data={\"connection_uri\":\"http://rabbitmq:15672\",\"username\":\"bao\",\"password\":\"...\"}; write path=roles/app data={\"vhosts\":{\"/\":{\"configure\":\".*\",\"write\":\".*\",\"read\":\".*\"}}}; read path=creds/app. Preserve native role tags, vhost topics and plugin fields in data; generated credentials and leases are returned directly.",
            true,
            "config/connection, config/lease, roles[/<role>], or creds/<role>, relative to the RabbitMQ secrets mount.",
        ),
        grouped(
            "totp_keys",
            "Create/import, list, read or delete native TOTP keys. Example write path=keys/alice data={\"generate\":true,\"issuer\":\"Example\",\"account_name\":\"alice@example.com\"}; list path=keys. Preserve all native generation, URL, algorithm and QR fields in data.",
            true,
            "keys or keys/<name>.",
        ),
        grouped(
            "totp_code",
            "Generate or validate a native TOTP code. Example read path=code/alice generates the current code; write path=code/alice data={\"code\":\"123456\"} validates it. Bao determines acceptance windows and replay behavior.",
            true,
            "code/<name>.",
        ),
        grouped(
            "cubbyhole",
            "Read/write/delete/list/scan caller-token-private native cubbyhole data. Example operation=write path=scratch data={\"value\":\"...\"}. The path is relative to cubbyhole; an empty path lists its root. Bao controls token lifetime and isolation.",
            false,
            "Path relative to cubbyhole; use an empty string only for root list/scan.",
        ),
        grouped(
            "identity",
            "Operate the native Identity API: entities/entity-aliases, groups/group-aliases, lookup, merge, OIDC keys/roles/providers/clients/assignments and identity tokens. Example write path=entity/name/alice data={\"metadata\":{\"team\":\"ops\"}}; write path=lookup/entity data={\"name\":\"alice\"}. Path is relative to identity; all native fields are preserved.",
            false,
            "Native identity-relative path, for example entity/id/<id>, group/name/team, lookup/entity, oidc/key/app, or oidc/token/app.",
        ),
        grouped(
            "auth",
            "Manage native auth mounts and call enabled auth backends, including their roles/configuration/users/groups/login endpoints. Paths include the native prefix: sys/auth (read), sys/auth/<mount> (write/delete), sys/auth/<mount>/tune, or auth/<mount>/... . Example write path=auth/userpass/users/alice data={\"password\":\"...\",\"token_policies\":[\"app\"]}. Backend requests still use the current caller token; any login response token is returned directly, never installed as a session credential.",
            false,
            "Full native path beginning sys/auth or auth. For token-specific endpoints prefer token.",
        ),
        grouped(
            "token",
            "Operate native auth/token endpoints: lookup-self/lookup/lookup-accessor, create[/role], roles, renew/renew-self, revoke/revoke-self/revoke-accessor/revoke-orphan and accessors. Examples read path=lookup-self; write path=create data={\"policies\":[\"app\"],\"ttl\":\"1h\"}; write path=revoke-accessor data={\"accessor\":\"...\"}. Tokens in bodies are native operation targets, never authentication-header overrides.",
            false,
            "Path relative to auth/token, for example create/app, roles/app, lookup-self or revoke-self.",
        ),
        grouped(
            "policy",
            "Read/list/write/delete native policies. Paths are relative to sys: policies/acl[/<name>], policy[/<name>] or policies/rgp|egp[/...] when supported. Example write path=policies/acl/app data={\"policy\":\"path ...\"}; list path=policies/acl. Bao parses policy syntax and enforces namespace boundaries.",
            false,
            "Native sys-relative policy path.",
        ),
        grouped(
            "leases",
            "Look up, list, renew, revoke or tidy native leases, including prefix/force revocation and count/list APIs. Example write path=renew data={\"lease_id\":\"database/creds/app/...\",\"increment\":3600}; list path=lookup/database/creds/app. Paths are relative to sys/leases; revoke-prefix and revoke-force can affect many leases.",
            false,
            "Native sys/leases-relative path: lookup[/prefix], renew[/lease_id], revoke[/lease_id], revoke-prefix/prefix, revoke-force/prefix, tidy, count; empty path addresses sys/leases.",
        ),
        grouped(
            "mounts",
            "Read, enable, tune, disable or remount native secrets engines. Paths are relative to sys: mounts[/<mount>[/tune]], remount or remount/status/<id>. Example write path=mounts/transit data={\"type\":\"transit\"}; read path=mounts. Disabling a mount deletes its data; it is an explicit native operation.",
            false,
            "Native sys-relative mounts or remount path.",
        ),
        grouped(
            "namespaces",
            "Manage native child namespaces beneath the configured namespace, including read/list/create/update/patch/delete and supported namespace lock/seal operations. Example list path=\"\"; write path=team data={}; read path=team. Path is relative to sys/namespaces. The gateway never accepts an alternative namespace header or creates namespaces at startup.",
            false,
            "Child namespace path relative to sys/namespaces; empty string addresses its collection. Native namespace action suffixes may follow the child path.",
        ),
        grouped(
            "audit",
            "Manage native audit devices, audit hashes and audited request-header settings. Paths are relative to sys: audit[/<device>], audit-hash/<device>, config/auditing/request-headers[/<header>]. Example write path=audit/file data={\"type\":\"file\",\"options\":{\"file_path\":\"/var/log/bao-audit.log\"}}. These are Bao audit operations; the gateway stores no business audit journal.",
            false,
            "Native sys-relative audit path.",
        ),
        grouped(
            "system",
            "Operate the remaining native sys APIs, including health/leader/HA, capabilities, wrapping, plugins, quotas, configuration, rotate/rekey/seal and storage diagnostics as authorized. Example write path=capabilities-self data={\"paths\":[\"team/data/app\"]}; write path=wrapping/unwrap data={}. Path is relative to sys; native body fields pass through unchanged. Prefer named families for mounts, leases, policies, namespaces, auth and audit.",
            false,
            "Native sys-relative path, for example health, leader, capabilities-self, wrapping/wrap, plugins/catalog/secret, or quotas/rate-limit/name.",
        ),
    ]
}

pub fn get(name: &str) -> Option<Definition> {
    all().into_iter().find(|tool| tool.name == name)
}

/// Family labels are discovery hints only. Every actual execution goes to Bao.
pub fn family(name: &str) -> Option<&'static str> {
    match name {
        "bao_request" | "api_help" => Some("generic"),
        "cache_refresh" => Some("cache"),
        "vault_create" => Some("vault"),
        "kv_read" | "kv_write" | "kv_manage" => Some("kv"),
        "ssh_ca" | "ssh_roles" | "ssh_issue" => Some("ssh"),
        "pki_ca" | "pki_roles" | "pki_certificates" | "pki_config" => Some("pki"),
        "transit_keys" | "transit_crypto" => Some("transit"),
        "database_connections" | "database_roles" | "database_credentials" => Some("database"),
        "kubernetes" => Some("kubernetes"),
        "ldap" => Some("ldap"),
        "rabbitmq" => Some("rabbitmq"),
        "totp_keys" | "totp_code" => Some("totp"),
        "cubbyhole" => Some("cubbyhole"),
        "identity" => Some("identity"),
        "auth" => Some("auth"),
        "token" => Some("token"),
        "policy" => Some("policy"),
        "leases" => Some("leases"),
        "mounts" => Some("mounts"),
        "namespaces" => Some("namespaces"),
        "audit" => Some("audit"),
        "system" => Some("system"),
        _ => None,
    }
}
