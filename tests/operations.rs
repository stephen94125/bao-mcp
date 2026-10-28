use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{
    Json, Router,
    body::to_bytes,
    extract::{Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::any,
};
use bao_mcp::{
    backend::{Backend, Credential},
    catalog, operations,
};
use serde_json::{Value, json};

struct Step {
    method: &'static str,
    path: String,
    query: Option<String>,
    body: Option<Value>,
    status: u16,
    response: Value,
}

impl Step {
    fn new(method: &'static str, path: &str, body: Option<Value>, response: Value) -> Self {
        Self {
            method,
            path: format!("/v1/{path}"),
            query: None,
            body,
            status: 200,
            response,
        }
    }
    fn query(mut self, query: &str) -> Self {
        self.query = Some(query.into());
        self
    }
    fn status(mut self, status: u16) -> Self {
        self.status = status;
        self
    }
}

struct Fixture {
    backend: Backend,
    credential: Credential,
    steps: Arc<Mutex<VecDeque<Step>>>,
    task: tokio::task::JoinHandle<()>,
}

impl Fixture {
    async fn new(steps: Vec<Step>) -> Self {
        let steps = Arc::new(Mutex::new(VecDeque::from(steps)));
        let app = Router::new().fallback(any(mock)).with_state(steps.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        Self {
            backend: Backend::new(
                &format!("http://{addr}"),
                "agent",
                None,
                false,
                Duration::from_secs(3),
            )
            .unwrap(),
            credential: Credential::new("synthetic-caller-token").unwrap(),
            steps,
            task,
        }
    }
    fn done(&self) {
        assert!(
            self.steps.lock().unwrap().is_empty(),
            "Unconsumed native requests"
        );
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn mock(State(steps): State<Arc<Mutex<VecDeque<Step>>>>, request: Request) -> Response {
    let step = steps
        .lock()
        .unwrap()
        .pop_front()
        .expect("Unexpected native request");
    let (parts, body) = request.into_parts();
    assert_eq!(parts.method.as_str(), step.method);
    assert_eq!(parts.uri.path(), step.path);
    assert_eq!(parts.uri.query(), step.query.as_deref());
    assert_eq!(parts.headers["x-vault-token"], "synthetic-caller-token");
    assert_eq!(parts.headers["x-vault-namespace"], "agent");
    let bytes = to_bytes(body, 1024 * 1024).await.unwrap();
    let body = if bytes.is_empty() {
        None
    } else {
        Some(serde_json::from_slice::<Value>(&bytes).unwrap())
    };
    assert_eq!(body, step.body);
    (
        StatusCode::from_u16(step.status).unwrap(),
        Json(step.response),
    )
        .into_response()
}

#[test]
fn catalog_has_complete_native_families_and_open_body_schemas() {
    let definitions = catalog::all();
    assert_eq!(definitions.len(), 34);
    let mut names = std::collections::BTreeSet::new();
    for definition in &definitions {
        assert!(names.insert(&definition.name));
        assert!(catalog::family(&definition.name).is_some());
        assert!(!definition.description.is_empty());
        assert_eq!(definition.input_schema["additionalProperties"], false);
        if let Some(data) = definition.input_schema["properties"].get("data") {
            assert_eq!(data["additionalProperties"], true);
        }
    }
    assert!(catalog::get("bao_request").is_some());
    assert!(catalog::get("unknown").is_none());
    assert!(catalog::family("unknown").is_none());
}

#[tokio::test]
async fn every_native_family_routes_with_caller_credentials() {
    let cases = [
        (
            "bao_request",
            json!({"operation":"write","path":"external-plugin/creds/web","data":{"custom_plugin_field":true}}),
            "PUT",
            "external-plugin/creds/web",
            Some(json!({"custom_plugin_field":true})),
        ),
        (
            "kv_read",
            json!({"mount":"team","path":"app"}),
            "GET",
            "team/data/app",
            None,
        ),
        (
            "kv_write",
            json!({"mount":"team","path":"app","data":{"data":{"nested":{"a":1}},"options":{"cas":0},"future_field":true}}),
            "PUT",
            "team/data/app",
            Some(json!({"data":{"nested":{"a":1}},"options":{"cas":0},"future_field":true})),
        ),
        (
            "kv_manage",
            json!({"operation":"write","mount":"team","path":"destroy/app","data":{"versions":[1,2]}}),
            "PUT",
            "team/destroy/app",
            Some(json!({"versions":[1,2]})),
        ),
        (
            "ssh_ca",
            json!({"operation":"write","mount":"ssh-ca-team","path":"config/ca","data":{"generate_signing_key":true}}),
            "PUT",
            "ssh-ca-team/config/ca",
            Some(json!({"generate_signing_key":true})),
        ),
        (
            "ssh_roles",
            json!({"operation":"read","mount":"ssh","path":"roles/server"}),
            "GET",
            "ssh/roles/server",
            None,
        ),
        (
            "ssh_issue",
            json!({"operation":"write","mount":"ssh","path":"sign/server","data":{"public_key":"ssh-ed25519 synthetic","valid_principals":"alice"}}),
            "PUT",
            "ssh/sign/server",
            Some(json!({"public_key":"ssh-ed25519 synthetic","valid_principals":"alice"})),
        ),
        (
            "pki_ca",
            json!({"operation":"write","mount":"_sys_pki_int","path":"issuer/default/sign-intermediate","data":{"csr":"synthetic-csr"}}),
            "PUT",
            "_sys_pki_int/issuer/default/sign-intermediate",
            Some(json!({"csr":"synthetic-csr"})),
        ),
        (
            "pki_roles",
            json!({"operation":"write","mount":"pki","path":"roles/web","data":{"allow_any_name":true,"future_role_option":7}}),
            "PUT",
            "pki/roles/web",
            Some(json!({"allow_any_name":true,"future_role_option":7})),
        ),
        (
            "pki_certificates",
            json!({"operation":"write","mount":"pki","path":"revoke","data":{"serial_number":"00:ab:cd"}}),
            "PUT",
            "pki/revoke",
            Some(json!({"serial_number":"00:ab:cd"})),
        ),
        (
            "pki_config",
            json!({"operation":"write","mount":"pki","path":"config/acme","data":{"enabled":true,"allowed_roles":["web"]}}),
            "PUT",
            "pki/config/acme",
            Some(json!({"enabled":true,"allowed_roles":["web"]})),
        ),
        (
            "transit_keys",
            json!({"operation":"write","mount":"transit","path":"keys/customer/import_version","data":{"ciphertext":"synthetic","hash_function":"SHA256"}}),
            "PUT",
            "transit/keys/customer/import_version",
            Some(json!({"ciphertext":"synthetic","hash_function":"SHA256"})),
        ),
        (
            "transit_crypto",
            json!({"operation":"write","mount":"transit","path":"encrypt/customer","data":{"batch_input":[{"plaintext":"YQ==","context":"Yg=="}]}}),
            "PUT",
            "transit/encrypt/customer",
            Some(json!({"batch_input":[{"plaintext":"YQ==","context":"Yg=="}]})),
        ),
        (
            "database_connections",
            json!({"operation":"write","mount":"db","path":"rotate-root/postgres"}),
            "PUT",
            "db/rotate-root/postgres",
            None,
        ),
        (
            "database_roles",
            json!({"operation":"read","mount":"db","path":"static-roles/app"}),
            "GET",
            "db/static-roles/app",
            None,
        ),
        (
            "database_credentials",
            json!({"operation":"read","mount":"db","path":"creds/app"}),
            "GET",
            "db/creds/app",
            None,
        ),
        (
            "kubernetes",
            json!({"operation":"write","mount":"k8s","path":"creds/app","data":{"kubernetes_namespace":"apps","ttl":"1h","extra_annotations":{"team":"ops"}}}),
            "PUT",
            "k8s/creds/app",
            Some(
                json!({"kubernetes_namespace":"apps","ttl":"1h","extra_annotations":{"team":"ops"}}),
            ),
        ),
        (
            "ldap",
            json!({"operation":"write","mount":"directory","path":"library/team/check-out","data":{"ttl":"1h","service_account_names":["alice"]}}),
            "PUT",
            "directory/library/team/check-out",
            Some(json!({"ttl":"1h","service_account_names":["alice"]})),
        ),
        (
            "rabbitmq",
            json!({"operation":"read","mount":"messages","path":"creds/app"}),
            "GET",
            "messages/creds/app",
            None,
        ),
        (
            "totp_keys",
            json!({"operation":"read","mount":"totp","path":"keys/alice"}),
            "GET",
            "totp/keys/alice",
            None,
        ),
        (
            "totp_code",
            json!({"operation":"write","mount":"totp","path":"code/alice","data":{"code":"123456"}}),
            "PUT",
            "totp/code/alice",
            Some(json!({"code":"123456"})),
        ),
        (
            "cubbyhole",
            json!({"operation":"write","path":"scratch","data":{"anything":[1,2]}}),
            "PUT",
            "cubbyhole/scratch",
            Some(json!({"anything":[1,2]})),
        ),
        (
            "identity",
            json!({"operation":"write","path":"entity/name/alice","data":{"metadata":{"team":"ops"}}}),
            "PUT",
            "identity/entity/name/alice",
            Some(json!({"metadata":{"team":"ops"}})),
        ),
        (
            "auth",
            json!({"operation":"write","path":"sys/auth/userpass","data":{"type":"userpass"}}),
            "PUT",
            "sys/auth/userpass",
            Some(json!({"type":"userpass"})),
        ),
        (
            "token",
            json!({"operation":"write","path":"create/app","data":{"policies":["app"],"ttl":"1h"}}),
            "PUT",
            "auth/token/create/app",
            Some(json!({"policies":["app"],"ttl":"1h"})),
        ),
        (
            "policy",
            json!({"operation":"write","path":"policies/acl/app","data":{"policy":"path \"team/data/app\" { capabilities = [\"read\"] }"}}),
            "PUT",
            "sys/policies/acl/app",
            Some(json!({"policy":"path \"team/data/app\" { capabilities = [\"read\"] }"})),
        ),
        (
            "leases",
            json!({"operation":"write","path":"renew","data":{"lease_id":"db/creds/app/one","increment":3600}}),
            "PUT",
            "sys/leases/renew",
            Some(json!({"lease_id":"db/creds/app/one","increment":3600})),
        ),
        (
            "mounts",
            json!({"operation":"write","path":"mounts/transit","data":{"type":"transit"}}),
            "PUT",
            "sys/mounts/transit",
            Some(json!({"type":"transit"})),
        ),
        (
            "namespaces",
            json!({"operation":"write","path":"child","data":{}}),
            "PUT",
            "sys/namespaces/child",
            Some(json!({})),
        ),
        (
            "audit",
            json!({"operation":"write","path":"audit/file","data":{"type":"file","options":{"file_path":"/tmp/audit.log"}}}),
            "PUT",
            "sys/audit/file",
            Some(json!({"type":"file","options":{"file_path":"/tmp/audit.log"}})),
        ),
        (
            "system",
            json!({"operation":"write","path":"capabilities-self","data":{"paths":["team/data/app"]}}),
            "PUT",
            "sys/capabilities-self",
            Some(json!({"paths":["team/data/app"]})),
        ),
    ];
    for (name, args, method, path, body) in cases {
        let response = json!({"request_id":"native","data":{"result":"unchanged"},"warnings":["native warning"]});
        let fixture = Fixture::new(vec![Step::new(method, path, body, response.clone())]).await;
        let result = operations::execute(&fixture.backend, &fixture.credential, name, args)
            .await
            .unwrap();
        assert_eq!(result, response, "{name}");
        fixture.done();
    }
}

#[tokio::test]
async fn kv_versions_cas_patch_and_v1_use_native_shapes() {
    let fixture = Fixture::new(vec![
        Step::new(
            "GET",
            "team/data/app",
            None,
            json!({"data":{"data":{"v":3},"metadata":{"version":3}}}),
        )
        .query("version=3"),
        Step::new(
            "GET",
            "team/metadata",
            None,
            json!({"data":{"keys":["app"]}}),
        )
        .query("list=true"),
        Step::new(
            "GET",
            "team/metadata/apps",
            None,
            json!({"data":{"keys":["api"]}}),
        )
        .query("scan=true"),
        Step::new(
            "PATCH",
            "team/data/app",
            Some(json!({"data":{"removed":null,"added":"v"},"options":{"cas":3}})),
            json!({"data":{"version":4}}),
        ),
        Step::new("PUT", "old/app", Some(json!({"plain":"v1"})), json!({})),
        Step::new("GET", "old", None, json!({"data":{"keys":["app"]}})).query("list=true"),
    ])
    .await;
    for (name, args) in [
        ("kv_read", json!({"mount":"team","path":"app","version":3})),
        (
            "kv_read",
            json!({"mount":"team","path":"","operation":"list"}),
        ),
        (
            "kv_read",
            json!({"mount":"team","path":"apps","operation":"scan"}),
        ),
        (
            "kv_write",
            json!({"mount":"team","path":"app","operation":"patch","data":{"data":{"removed":null,"added":"v"},"options":{"cas":3}}}),
        ),
        (
            "kv_write",
            json!({"mount":"old","path":"app","engine_version":1,"data":{"plain":"v1"}}),
        ),
        (
            "kv_read",
            json!({"mount":"old","path":"","engine_version":1,"operation":"list"}),
        ),
    ] {
        operations::execute(&fixture.backend, &fixture.credential, name, args)
            .await
            .unwrap();
    }
    fixture.done();
}

#[tokio::test]
async fn generic_escape_hatch_supports_every_native_operation() {
    let operations_and_methods = [
        ("read", "GET"),
        ("write", "PUT"),
        ("create", "PUT"),
        ("update", "PUT"),
        ("patch", "PATCH"),
        ("delete", "DELETE"),
        ("list", "GET"),
        ("scan", "GET"),
    ];
    let steps = operations_and_methods
        .iter()
        .map(|(operation, method)| {
            let mut step = Step::new(
                method,
                "plugin/arbitrary",
                Some(json!({"future":[1,{"enabled":true}]})),
                json!({"ok":true}),
            );
            if *operation == "list" {
                step = step.query("list=true");
            }
            if *operation == "scan" {
                step = step.query("scan=true");
            }
            step
        })
        .collect();
    let fixture = Fixture::new(steps).await;
    for (operation, _) in operations_and_methods {
        operations::execute(&fixture.backend, &fixture.credential, "bao_request", json!({"operation":operation,"path":"plugin/arbitrary","data":{"future":[1,{"enabled":true}]}})).await.unwrap();
    }
    fixture.done();
}

#[tokio::test]
async fn native_help_and_full_openapi_are_live_requests() {
    let fixture = Fixture::new(vec![
        Step::new(
            "GET",
            "transit/encrypt/customer",
            None,
            json!({"help":"native help","openapi":{}}),
        )
        .query("help=1"),
        Step::new(
            "GET",
            "sys/internal/specs/openapi",
            None,
            json!({"openapi":"3.0.2"}),
        )
        .query("generic_mount_paths=true"),
    ])
    .await;
    let result = operations::execute(
        &fixture.backend,
        &fixture.credential,
        "api_help",
        json!({"path":"transit/encrypt/customer"}),
    )
    .await
    .unwrap();
    assert_eq!(result["help"], "native help");
    operations::execute(
        &fixture.backend,
        &fixture.credential,
        "api_help",
        json!({"generic_mount_paths":true}),
    )
    .await
    .unwrap();
    fixture.done();
}

#[tokio::test]
async fn invalid_arguments_never_reach_the_backend() {
    let fixture = Fixture::new(vec![]).await;
    let cases = [
        ("unknown", json!({})),
        ("bao_request", json!([])),
        (
            "bao_request",
            json!({"operation":"execute","path":"sys/health"}),
        ),
        ("bao_request", json!({"operation":"read"})),
        ("bao_request", json!({"operation":"read","path":42})),
        (
            "bao_request",
            json!({"operation":"read","path":"sys/health","token":"override"}),
        ),
        (
            "bao_request",
            json!({"operation":"read","path":"https://evil.example/path"}),
        ),
        (
            "bao_request",
            json!({"operation":"read","path":"/sys/health"}),
        ),
        (
            "bao_request",
            json!({"operation":"read","path":"sys/../health"}),
        ),
        (
            "bao_request",
            json!({"operation":"read","path":"sys/%2e%2e/health"}),
        ),
        (
            "bao_request",
            json!({"operation":"read","path":"sys/health?namespace=root"}),
        ),
        (
            "bao_request",
            json!({"operation":"read","path":"sys/health","query":{"bad":[]}}),
        ),
        (
            "bao_request",
            json!({"operation":"write","path":"plugin/x","data":[]}),
        ),
        (
            "bao_request",
            json!({"operation":"read","path":"sys/health","wrap_ttl":"1s\r\nX: y"}),
        ),
        ("kv_read", json!({"mount":"team","path":"app","version":0})),
        ("kv_read", json!({"mount":"team","path":"app","version":-1})),
        (
            "kv_read",
            json!({"mount":"team","path":"app","engine_version":1,"version":1}),
        ),
        (
            "kv_read",
            json!({"mount":"team","path":"app","version":1,"query":{"version":2}}),
        ),
        (
            "kv_read",
            json!({"mount":"team","path":"","operation":"read"}),
        ),
        ("kv_read", json!({"mount":"team/escape","path":"app"})),
        (
            "kv_write",
            json!({"mount":"team","path":"app","data":{"password":"missing native envelope"}}),
        ),
        (
            "kv_write",
            json!({"mount":"old","path":"app","engine_version":1,"operation":"patch","data":{}}),
        ),
        (
            "ssh_roles",
            json!({"operation":"write","mount":"ssh","path":"sign/web","data":{}}),
        ),
        (
            "pki_config",
            json!({"operation":"write","mount":"pki","path":"root/generate/internal","data":{}}),
        ),
        (
            "auth",
            json!({"operation":"write","path":"sys/mounts/auth","data":{}}),
        ),
        (
            "kubernetes",
            json!({"operation":"write","mount":"k8s","path":"root/generate/internal","data":{}}),
        ),
        (
            "ldap",
            json!({"operation":"write","mount":"directory","path":"roles/app","data":{}}),
        ),
        (
            "rabbitmq",
            json!({"operation":"write","mount":"messages","path":"config/unsupported","data":{}}),
        ),
        ("vault_create", json!({"vault_id":"../escape"})),
        ("cache_refresh", json!({"token":"override"})),
    ];
    for (name, args) in cases {
        let error = operations::execute(&fixture.backend, &fixture.credential, name, args.clone())
            .await
            .unwrap_err();
        assert_eq!(error.status, 400, "{name}: {args}");
    }
    fixture.done();
}

fn absent(path: &str) -> Step {
    Step::new("GET", path, None, json!({"errors":["not found"]})).status(404)
}

fn existing(path: &str, kind: &str) -> Step {
    let mut data = json!({"type":kind,"description":"Retain this exact setting"});
    if kind == "kv" {
        data["options"] = json!({"version":"2"});
    }
    Step::new("GET", path, None, json!({"data":data}))
}

#[tokio::test]
async fn vault_create_allocates_only_missing_mounts_and_never_generates_ca() {
    let mounts = [
        ("team", "kv"),
        ("ssh-ca-team", "ssh"),
        ("_sys_pki_root", "pki"),
        ("_sys_pki_int", "pki"),
    ];
    let mut steps = Vec::new();
    for (mount, kind) in mounts {
        let path = format!("sys/mounts/{mount}");
        steps.push(absent(&path));
        let mut body = json!({"type":kind,"description":"Bundle description"});
        if kind == "kv" {
            body["options"] = json!({"version":"2"});
        }
        steps.push(Step::new("PUT", &path, Some(body), json!({})));
    }
    for (mount, kind) in mounts {
        steps.push(existing(&format!("sys/mounts/{mount}"), kind));
    }
    let fixture = Fixture::new(steps).await;
    let created = operations::execute(
        &fixture.backend,
        &fixture.credential,
        "vault_create",
        json!({"vault_id":"team","description":"Bundle description"}),
    )
    .await
    .unwrap();
    assert_eq!(created["complete"], true);
    assert_eq!(created["ca_initialization_performed"], false);
    assert!(
        created["mounts"]
            .as_array()
            .unwrap()
            .iter()
            .all(|m| m["status"] == "created")
    );
    assert_eq!(
        created["allocation"]["pki_scope"],
        "shared within the configured namespace"
    );
    let retained = operations::execute(
        &fixture.backend,
        &fixture.credential,
        "vault_create",
        json!({"vault_id":"team","description":"Must not rewrite existing config"}),
    )
    .await
    .unwrap();
    assert_eq!(retained["complete"], true);
    assert!(
        retained["mounts"]
            .as_array()
            .unwrap()
            .iter()
            .all(|m| m["status"] == "existing")
    );
    fixture.done();
}

#[tokio::test]
async fn vault_create_reports_partial_native_failure_without_rollback_or_later_writes() {
    let fixture = Fixture::new(vec![
        absent("sys/mounts/team"),
        Step::new(
            "PUT",
            "sys/mounts/team",
            Some(json!({"type":"kv","options":{"version":"2"}})),
            json!({}),
        ),
        absent("sys/mounts/ssh-ca-team"),
        Step::new(
            "PUT",
            "sys/mounts/ssh-ca-team",
            Some(json!({"type":"ssh"})),
            json!({"errors":["sensitive native message"]}),
        )
        .status(403),
    ])
    .await;
    let result = operations::execute(
        &fixture.backend,
        &fixture.credential,
        "vault_create",
        json!({"vault_id":"team"}),
    )
    .await
    .unwrap();
    assert_eq!(result["complete"], false);
    assert_eq!(result["status"], "partial");
    assert_eq!(result["mounts"][0]["status"], "created");
    assert_eq!(result["mounts"][1]["status"], "failed");
    assert_eq!(result["mounts"][2]["status"], "not_attempted");
    assert_eq!(result["mounts"][3]["status"], "not_attempted");
    assert_eq!(result["error"]["status"], 403);
    assert!(!result.to_string().contains("sensitive native message"));
    fixture.done();
}

#[tokio::test]
async fn vault_create_never_rewrites_conflicting_mounts_or_treats_denial_as_absence() {
    for first in [
        existing("sys/mounts/team", "transit"),
        Step::new(
            "GET",
            "sys/mounts/team",
            None,
            json!({"data":{"type":"kv","options":{"version":"1"}}}),
        ),
        Step::new(
            "GET",
            "sys/mounts/team",
            None,
            json!({"errors":["forbidden"]}),
        )
        .status(403),
    ] {
        let fixture = Fixture::new(vec![first]).await;
        let result = operations::execute(
            &fixture.backend,
            &fixture.credential,
            "vault_create",
            json!({"vault_id":"team"}),
        )
        .await
        .unwrap();
        assert_eq!(result["complete"], false);
        assert_eq!(result["status"], "failed");
        assert_eq!(result["mounts"][1]["status"], "not_attempted");
        fixture.done();
    }
}

#[tokio::test]
async fn vault_create_legacy_mount_probe_confirms_absence_before_enabling() {
    let fixture = Fixture::new(vec![
        Step::new(
            "GET",
            "sys/mounts/team",
            None,
            json!({"errors":["ambiguous native rejection"]}),
        )
        .status(400),
        Step::new(
            "GET",
            "sys/mounts",
            None,
            json!({"data":{
                "ssh-ca-team/":{"type":"ssh"},
                "_sys_pki_root/":{"type":"pki"},
                "_sys_pki_int/":{"type":"pki"}
            }}),
        ),
        Step::new(
            "PUT",
            "sys/mounts/team",
            Some(json!({"type":"kv","options":{"version":"2"}})),
            json!({}),
        ),
        existing("sys/mounts/ssh-ca-team", "ssh"),
        existing("sys/mounts/_sys_pki_root", "pki"),
        existing("sys/mounts/_sys_pki_int", "pki"),
    ])
    .await;
    let result = operations::execute(
        &fixture.backend,
        &fixture.credential,
        "vault_create",
        json!({"vault_id":"team"}),
    )
    .await
    .unwrap();
    assert_eq!(result["complete"], true);
    assert_eq!(result["mounts"][0]["status"], "created");
    assert!(
        result["mounts"].as_array().unwrap()[1..]
            .iter()
            .all(|entry| entry["status"] == "existing")
    );
    fixture.done();
}

#[tokio::test]
async fn vault_create_legacy_mount_probe_uses_existing_catalog_metadata() {
    let fixture = Fixture::new(vec![
        Step::new(
            "GET",
            "sys/mounts/team",
            None,
            json!({"errors":["unsupported individual probe"]}),
        )
        .status(400),
        Step::new(
            "GET",
            "sys/mounts",
            None,
            json!({"data":{"team/":{"type":"kv","options":{"version":"2"}}}}),
        ),
        existing("sys/mounts/ssh-ca-team", "ssh"),
        existing("sys/mounts/_sys_pki_root", "pki"),
        existing("sys/mounts/_sys_pki_int", "pki"),
    ])
    .await;
    let result = operations::execute(
        &fixture.backend,
        &fixture.credential,
        "vault_create",
        json!({"vault_id":"team"}),
    )
    .await
    .unwrap();
    assert_eq!(result["complete"], true);
    assert!(
        result["mounts"]
            .as_array()
            .unwrap()
            .iter()
            .all(|entry| entry["status"] == "existing")
    );
    fixture.done();
}

#[tokio::test]
async fn vault_create_legacy_mount_probe_never_enables_after_catalog_denial_or_malformed_response()
{
    for (catalog, status) in [
        (
            Step::new("GET", "sys/mounts", None, json!({"errors":["denied"]})).status(403),
            403,
        ),
        (
            Step::new("GET", "sys/mounts", None, json!({"data":[]})),
            502,
        ),
        (
            Step::new(
                "GET",
                "sys/mounts",
                None,
                json!({"data":{"unexpected":true}}),
            ),
            502,
        ),
    ] {
        let fixture = Fixture::new(vec![
            Step::new(
                "GET",
                "sys/mounts/team",
                None,
                json!({"errors":["ambiguous native rejection"]}),
            )
            .status(400),
            catalog,
        ])
        .await;
        let result = operations::execute(
            &fixture.backend,
            &fixture.credential,
            "vault_create",
            json!({"vault_id":"team"}),
        )
        .await
        .unwrap();
        assert_eq!(result["complete"], false);
        assert_eq!(result["error"]["status"], status);
        assert_eq!(result["mounts"][0]["status"], "failed");
        assert!(
            result["mounts"].as_array().unwrap()[1..]
                .iter()
                .all(|entry| entry["status"] == "not_attempted")
        );
        fixture.done();
    }
}
