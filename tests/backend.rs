use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use axum::{
    Json, Router,
    body::{Body, to_bytes},
    extract::{Request, State},
    http::{HeaderMap, Method, StatusCode, Uri},
    response::Response,
    routing::{any, get},
};
use bao_mcp::backend::{Backend, Credential};
use serde_json::{Value, json};
use tokio::{net::TcpListener, task::JoinHandle};

struct TestServer {
    address: String,
    task: JoinHandle<()>,
}

impl TestServer {
    async fn start(router: Router) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        Self { address, task }
    }

    fn backend(&self, namespace: &str) -> Backend {
        Backend::new(
            &self.address,
            namespace,
            None,
            false,
            Duration::from_secs(3),
        )
        .unwrap()
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[derive(Debug)]
struct Captured {
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Value,
}

async fn capture(
    State(captured): State<Arc<Mutex<Vec<Captured>>>>,
    request: Request,
) -> Json<Value> {
    let (parts, body) = request.into_parts();
    let body = to_bytes(body, 1024 * 1024).await.unwrap();
    captured.lock().unwrap().push(Captured {
        method: parts.method,
        uri: parts.uri,
        headers: parts.headers,
        body: serde_json::from_slice(&body).unwrap_or(Value::Null),
    });
    Json(json!({ "data": { "secret": "returned-to-authorized-caller" } }))
}

fn capture_router(captured: Arc<Mutex<Vec<Captured>>>) -> Router {
    Router::new()
        .route("/v1/{*path}", any(capture))
        .with_state(captured)
}

#[tokio::test]
async fn concurrent_callers_keep_tokens_request_scoped_and_namespace_fixed() {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let server = TestServer::start(capture_router(captured.clone())).await;
    let backend = server.backend("/agent/team/");
    let first = Credential::new("test-caller-one").unwrap();
    let second = Credential::new("test-caller-two").unwrap();
    let query = vec![("name".into(), "value&namespace=outside".into())];
    let (first_result, second_result) = tokio::join!(
        backend.request(&first, "read", "secret/data/one", None, &query, Some("5m")),
        backend.request(&second, "read", "secret/data/two", None, &[], None),
    );
    assert_eq!(
        first_result.unwrap()["data"]["secret"],
        "returned-to-authorized-caller"
    );
    second_result.unwrap();
    assert_eq!(backend.namespace(), "agent/team");

    let captured = captured.lock().unwrap();
    assert_eq!(captured.len(), 2);
    for request in captured.iter() {
        assert_eq!(request.headers["x-vault-namespace"], "agent/team");
        assert!(!request.headers.contains_key("authorization"));
        match request.uri.path() {
            "/v1/secret/data/one" => {
                assert_eq!(request.headers["x-vault-token"], "test-caller-one");
                assert_eq!(request.headers["x-vault-wrap-ttl"], "5m");
                let url = url::Url::parse(&format!("http://test{}", request.uri)).unwrap();
                assert_eq!(
                    url.query_pairs().collect::<Vec<_>>(),
                    vec![("name".into(), "value&namespace=outside".into())]
                );
            }
            "/v1/secret/data/two" => {
                assert_eq!(request.headers["x-vault-token"], "test-caller-two");
                assert!(!request.headers.contains_key("x-vault-wrap-ttl"));
            }
            other => panic!("unexpected request path: {other}"),
        }
    }
}

#[tokio::test]
async fn logical_operations_map_to_native_http_semantics() {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let server = TestServer::start(capture_router(captured.clone())).await;
    let backend = server.backend("");
    let credential = Credential::new("test-token").unwrap();
    let operations = [
        "read", "write", "create", "update", "patch", "delete", "list", "scan",
    ];
    for operation in operations {
        let body = matches!(operation, "write" | "create" | "update" | "patch")
            .then(|| json!({ "data": { "key": "value" } }));
        backend
            .request(&credential, operation, "secret/data/test", body, &[], None)
            .await
            .unwrap();
    }
    let captured = captured.lock().unwrap();
    let methods: Vec<_> = captured
        .iter()
        .map(|request| request.method.clone())
        .collect();
    assert_eq!(
        methods,
        [
            Method::GET,
            Method::PUT,
            Method::PUT,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
            Method::GET,
            Method::GET
        ]
    );
    assert_eq!(captured[6].uri.query(), Some("list=true"));
    assert_eq!(captured[7].uri.query(), Some("scan=true"));
    assert_eq!(
        captured[4].headers["content-type"],
        "application/merge-patch+json"
    );
    assert_eq!(
        captured[4].headers.get_all("content-type").iter().count(),
        1
    );
    assert_eq!(captured[4].body["data"]["key"], "value");
    assert!(
        captured
            .iter()
            .all(|request| !request.headers.contains_key("x-vault-namespace"))
    );
}

#[tokio::test]
async fn unsafe_paths_and_operation_overrides_are_rejected_before_http() {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let server = TestServer::start(capture_router(captured.clone())).await;
    let backend = server.backend("agent");
    let credential = Credential::new("test-token").unwrap();
    for path in [
        "",
        "/sys/mounts",
        "//example.test/path",
        "https://example.test/path",
        "https:example.test",
        "secret/../sys/mounts",
        "secret/./test",
        "secret//test",
        "secret/test//",
        "secret/%2e%2e/sys",
        "secret/%2Fsys",
        "secret/%252f",
        "secret/test?namespace=root",
        "secret/test#fragment",
        "secret\\..\\sys",
        "secret/test\r\nX-Vault-Namespace: root",
    ] {
        let error = backend
            .request(&credential, "read", path, None, &[], None)
            .await
            .unwrap_err();
        assert_eq!(error.status, 400, "accepted unsafe path: {path}");
    }
    for selector in ["list", "scan"] {
        let error = backend
            .request(
                &credential,
                "read",
                "sys/mounts",
                None,
                &[(selector.into(), "true".into())],
                None,
            )
            .await
            .unwrap_err();
        assert_eq!(error.status, 400);
    }
    for ttl in [
        "",
        "0s",
        "-1m",
        "1h30m",
        "1m\r\nX-Vault-Namespace: root",
        "1e9s",
    ] {
        let error = backend
            .request(&credential, "read", "sys/mounts", None, &[], Some(ttl))
            .await
            .unwrap_err();
        assert_eq!(error.status, 400);
    }
    assert!(captured.lock().unwrap().is_empty());
}

#[test]
fn credentials_and_config_validate_without_revealing_token_values() {
    for token in [
        "",
        "contains space",
        "test\r\nheader",
        "test\ttoken",
        "中文",
    ] {
        let error = Credential::new(token).unwrap_err();
        assert_eq!(error.status, 401);
        assert!(!error.message.contains(token) || token.is_empty());
    }
    assert!(Credential::new(&"a".repeat(16 * 1024 + 1)).is_err());
    let first = Credential::new("test-caller-one").unwrap();
    let second = Credential::new("test-caller-two").unwrap();
    assert_ne!(first.hash(), second.hash());
    assert_eq!(format!("{first:?}"), "Credential([REDACTED])");
    for address in [
        "file:///tmp/bao",
        "ftp://example.test",
        "http://user:password@example.test",
        "http://@example.test",
        "http://example.test?token=secret",
        "http://example.test#fragment",
        "http://example.test/v1/",
        "http://example.test/base",
        " http://example.test",
        "http://example.test\n",
    ] {
        assert!(Backend::new(address, "agent", None, false, Duration::from_secs(1)).is_err());
    }
    for namespace in [
        "agent\r\nX-Vault-Token: fake",
        "agent//other",
        "agent/../root",
        "agent%2froot",
    ] {
        assert!(
            Backend::new(
                "http://127.0.0.1",
                namespace,
                None,
                false,
                Duration::from_secs(1)
            )
            .is_err()
        );
    }
    assert!(Backend::new("http://127.0.0.1", "agent", None, false, Duration::ZERO).is_err());
}

#[test]
fn scope_hash_uses_canonical_origin_and_fixed_namespace() {
    let canonical = Backend::new(
        "http://example.test",
        "agent",
        None,
        false,
        Duration::from_secs(1),
    )
    .unwrap();
    let equivalent = Backend::new(
        "HTTP://EXAMPLE.TEST:80/",
        "/agent/",
        None,
        false,
        Duration::from_secs(1),
    )
    .unwrap();
    let other = Backend::new(
        "http://example.test",
        "other",
        None,
        false,
        Duration::from_secs(1),
    )
    .unwrap();
    let root = Backend::new(
        "http://example.test",
        "",
        None,
        false,
        Duration::from_secs(1),
    )
    .unwrap();
    assert_eq!(canonical.scope_id(), equivalent.scope_id());
    assert_ne!(canonical.scope_id(), other.scope_id());
    assert_ne!(canonical.scope_id(), root.scope_id());
}

#[tokio::test]
async fn redirects_are_not_followed_or_allowed_to_receive_credentials() {
    let redirected_requests = Arc::new(AtomicUsize::new(0));
    let count = redirected_requests.clone();
    let destination = TestServer::start(Router::new().route(
        "/stolen",
        get(move || {
            let count = count.clone();
            async move {
                count.fetch_add(1, Ordering::SeqCst);
                Json(json!({ "unexpected": true }))
            }
        }),
    ))
    .await;
    let location = format!("{}/stolen", destination.address);
    let source = TestServer::start(Router::new().route(
        "/v1/secret/test",
        get(move || {
            let location = location.clone();
            async move {
                Response::builder()
                    .status(StatusCode::TEMPORARY_REDIRECT)
                    .header("Location", location)
                    .body(Body::empty())
                    .unwrap()
            }
        }),
    ))
    .await;
    let error = source
        .backend("agent")
        .request(
            &Credential::new("test-token").unwrap(),
            "read",
            "secret/test",
            None,
            &[],
            None,
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, "backend_redirect");
    assert_eq!(redirected_requests.load(Ordering::SeqCst), 0);
    assert!(!error.message.contains(&destination.address));
}

#[tokio::test]
async fn upstream_errors_preserve_status_without_echoing_secret_bodies_or_headers() {
    let requests = Arc::new(AtomicUsize::new(0));
    let count = requests.clone();
    let server = TestServer::start(Router::new().route(
        "/v1/fail/{status}",
        any(move |request: Request| {
            let count = count.clone();
            async move {
                count.fetch_add(1, Ordering::SeqCst);
                let status = request
                    .uri()
                    .path()
                    .rsplit('/')
                    .next()
                    .unwrap()
                    .parse::<u16>()
                    .unwrap();
                Response::builder()
                    .status(status)
                    .header("X-Upstream-Secret", "private-header-value")
                    .body(Body::from(
                        r#"{"errors":["private-error-body"],"token":"private-token"}"#,
                    ))
                    .unwrap()
            }
        }),
    ))
    .await;
    let backend = server.backend("agent");
    let credential = Credential::new("test-token").unwrap();
    for (status, code) in [
        (401, "unauthorized"),
        (403, "permission_denied"),
        (404, "not_found"),
        (429, "rate_limited"),
        (500, "backend_unavailable"),
        (503, "backend_unavailable"),
    ] {
        let error = backend
            .request(
                &credential,
                "read",
                &format!("fail/{status}"),
                None,
                &[],
                None,
            )
            .await
            .unwrap_err();
        assert_eq!(error.status, status);
        assert_eq!(error.code, code);
        assert!(!serde_json::to_string(&error).unwrap().contains("private-"));
    }
    for operation in ["write", "create", "update", "patch", "delete"] {
        let error = backend
            .request(
                &credential,
                operation,
                "fail/503",
                Some(json!({})),
                &[],
                None,
            )
            .await
            .unwrap_err();
        assert_eq!(error.status, 503);
        assert_eq!(error.code, "outcome_unknown");
        assert!(!serde_json::to_string(&error).unwrap().contains("private-"));
    }
    assert_eq!(requests.load(Ordering::SeqCst), 11);
}

#[tokio::test]
async fn timeouts_report_unknown_mutation_outcome_without_retry() {
    let count = Arc::new(AtomicUsize::new(0));
    let requests = count.clone();
    let server = TestServer::start(Router::new().route(
        "/v1/slow",
        any(move || {
            let requests = requests.clone();
            async move {
                requests.fetch_add(1, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(250)).await;
                Json(json!({ "data": {} }))
            }
        }),
    ))
    .await;
    let backend = Backend::new(
        &server.address,
        "agent",
        None,
        false,
        Duration::from_millis(50),
    )
    .unwrap();
    let credential = Credential::new("test-token").unwrap();
    let read = backend
        .request(&credential, "read", "slow", None, &[], None)
        .await
        .unwrap_err();
    assert_eq!(read.status, 504);
    assert_eq!(read.code, "backend_timeout");
    let write = backend
        .request(&credential, "write", "slow", Some(json!({})), &[], None)
        .await
        .unwrap_err();
    assert_eq!(write.code, "outcome_unknown");
    assert_eq!(count.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn success_payloads_keep_json_pem_binary_and_empty_response_semantics() {
    let server = TestServer::start(
        Router::new()
            .route(
                "/v1/json",
                get(|| async {
                    Json(json!({ "data": { "private_key": "caller-visible-secret" } }))
                }),
            )
            .route(
                "/v1/pem",
                get(|| async {
                    Response::builder()
                        .header("Content-Type", "application/pem-certificate-chain")
                        .body(Body::from(
                            "-----BEGIN CERTIFICATE-----\nexample\n-----END CERTIFICATE-----\n",
                        ))
                        .unwrap()
                }),
            )
            .route(
                "/v1/binary",
                get(|| async {
                    Response::builder()
                        .header("Content-Type", "application/octet-stream")
                        .body(Body::from(vec![0, 255, 16]))
                        .unwrap()
                }),
            )
            .route("/v1/empty", get(|| async { StatusCode::NO_CONTENT }))
            .route(
                "/v1/invalid-json",
                get(|| async {
                    Response::builder()
                        .header("Content-Type", "application/json")
                        .body(Body::from("private-invalid-json-body"))
                        .unwrap()
                }),
            ),
    )
    .await;
    let backend = server.backend("agent");
    let credential = Credential::new("test-token").unwrap();
    let json = backend
        .request(&credential, "read", "json", None, &[], None)
        .await
        .unwrap();
    assert_eq!(json["data"]["private_key"], "caller-visible-secret");
    let pem = backend
        .request(&credential, "read", "pem", None, &[], None)
        .await
        .unwrap();
    assert_eq!(pem["content_type"], "application/pem-certificate-chain");
    assert!(
        pem["body"]
            .as_str()
            .unwrap()
            .starts_with("-----BEGIN CERTIFICATE-----")
    );
    let binary = backend
        .request(&credential, "read", "binary", None, &[], None)
        .await
        .unwrap();
    assert_eq!(binary["encoding"], "base64");
    assert_eq!(binary["body"], "AP8Q");
    assert_eq!(
        backend
            .request(&credential, "read", "empty", None, &[], None)
            .await
            .unwrap(),
        Value::Null
    );
    let error = backend
        .request(&credential, "read", "invalid-json", None, &[], None)
        .await
        .unwrap_err();
    assert_eq!(error.code, "backend_invalid_response");
    assert!(!error.message.contains("private-"));
}

#[tokio::test]
async fn response_limit_applies_with_and_without_content_length() {
    let oversized = Arc::new(vec![b'a'; 8 * 1024 * 1024 + 1]);
    let hinted = oversized.clone();
    let chunked = oversized.clone();
    let server = TestServer::start(
        Router::new()
            .route(
                "/v1/hinted",
                get(move || {
                    let body = hinted.clone();
                    async move {
                        Response::builder()
                            .body(Body::from(body.as_ref().clone()))
                            .unwrap()
                    }
                }),
            )
            .route(
                "/v1/chunked",
                get(move || {
                    let body = chunked.clone();
                    async move {
                        Response::builder()
                            .body(Body::from_stream(tokio_stream::iter([Ok::<
                                _,
                                std::io::Error,
                            >(
                                body.as_ref().clone(),
                            )])))
                            .unwrap()
                    }
                }),
            ),
    )
    .await;
    let backend = server.backend("agent");
    let credential = Credential::new("test-token").unwrap();
    for path in ["hinted", "chunked"] {
        let error = backend
            .request(&credential, "read", path, None, &[], None)
            .await
            .unwrap_err();
        assert_eq!(
            error.code, "backend_response_too_large",
            "failed bound for {path}"
        );
    }
}
