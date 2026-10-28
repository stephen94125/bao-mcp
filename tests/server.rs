use axum::{
    Json, Router,
    extract::{Request, State},
    response::IntoResponse,
};
use bao_mcp::{
    config::Config,
    server::{App, router},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio_stream::StreamExt;
use tower::ServiceExt;
#[derive(Clone, Default)]
struct Counts {
    lookup: Arc<AtomicUsize>,
    requests: Arc<AtomicUsize>,
}
async fn upstream(State(counts): State<Counts>, request: Request) -> axum::response::Response {
    assert_eq!(request.headers().get("x-vault-namespace").unwrap(), "agent");
    let token = request
        .headers()
        .get("x-vault-token")
        .unwrap()
        .to_str()
        .unwrap();
    if !["synthetic-client-a", "synthetic-client-b"].contains(&token) {
        return (
            http::StatusCode::FORBIDDEN,
            Json(json!({"errors":["sensitive diagnostic"]})),
        )
            .into_response();
    }
    match request.uri().path() {
        "/v1/auth/token/lookup-self" => {
            counts.lookup.fetch_add(1, Ordering::SeqCst);
            Json(json!({"data":{"id":token,"ttl":3600}})).into_response()
        }
        "/v1/sys/internal/ui/mounts" => {
            Json(json!({"data":{"secret":{"secret/":{"type":"kv","options":{"version":"2"}}}}}))
                .into_response()
        }
        "/v1/sys/internal/ui/resultant-acl" => {
            Json(json!({"data":{"root":true,"exact_paths":{},"glob_paths":{}}})).into_response()
        }
        "/v1/sys/capabilities-self" => {
            let bytes = request.into_body().collect().await.unwrap().to_bytes();
            let body: Value = serde_json::from_slice(&bytes).unwrap();
            let map: serde_json::Map<String, Value> = body["paths"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| (p.as_str().unwrap().into(), json!(["root"])))
                .collect();
            Json(json!({"data":map})).into_response()
        }
        "/v1/secret/data/allowed" => {
            counts.requests.fetch_add(1, Ordering::SeqCst);
            if request.method() == http::Method::PUT {
                let bytes = request.into_body().collect().await.unwrap().to_bytes();
                if serde_json::from_slice::<Value>(&bytes).unwrap()["slow"] == true {
                    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
                }
                return Json(json!({"data":{"received_bytes":bytes.len()}})).into_response();
            }
            Json(json!({"data":{"data":{"client":if token.ends_with('a'){"A"}else{"B"}},"metadata":{"version":1}}})).into_response()
        }
        "/v1/secret/data/denied" => {
            (http::StatusCode::FORBIDDEN, Json(json!({"errors":[token]}))).into_response()
        }
        _ => (
            http::StatusCode::NOT_FOUND,
            Json(json!({"errors":["not found"]})),
        )
            .into_response(),
    }
}
async fn fixture() -> (
    tempfile::TempDir,
    Router,
    Counts,
    tokio::task::JoinHandle<()>,
) {
    fixture_with_body_timeout(30).await
}
async fn fixture_with_body_timeout(
    seconds: u64,
) -> (
    tempfile::TempDir,
    Router,
    Counts,
    tokio::task::JoinHandle<()>,
) {
    let counts = Counts::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = Router::new().fallback(upstream).with_state(counts.clone());
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().canonicalize().unwrap().join("cache.db");
    let config = Config::from_values(|key| match key {
        "BAO_ADDR" => Some(format!("http://{addr}")),
        "CACHE_PATH" => Some(path.to_string_lossy().into_owned()),
        "MCP_BODY_TIMEOUT_SECONDS" => Some(seconds.to_string()),
        _ => None,
    })
    .unwrap();
    let app = App::new(&config).unwrap();
    (temp, router(app, &config), counts, handle)
}
async fn rpc(app: &Router, token: Option<&str>, method: &str, params: Value) -> (u16, Value) {
    let mut request = http::Request::builder()
        .method("POST")
        .uri("/mcp")
        .header("host", "localhost")
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .header("mcp-protocol-version", "2025-06-18");
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    let response = app
        .clone()
        .oneshot(
            request
                .body(axum::body::Body::from(
                    json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status().as_u16();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body = serde_json::from_slice(&bytes)
        .unwrap_or_else(|_| json!({"raw":String::from_utf8_lossy(&bytes)}));
    (status, body)
}
#[tokio::test]
async fn token_pass_through_catalog_cache_and_errors_at_real_mcp_boundary() {
    let (_temp, app, counts, handle) = fixture().await;
    let (status, _) = rpc(&app, None, "tools/list", json!({})).await;
    assert_eq!(status, 401);
    let (status, body) = rpc(&app, Some("invalid-client"), "tools/list", json!({})).await;
    assert_eq!(status, 403);
    assert!(!body.to_string().contains("sensitive diagnostic"));
    for token in ["synthetic-client-a", "synthetic-client-b"] {
        let (status, body) = rpc(&app, Some(token), "tools/list", json!({})).await;
        assert_eq!(status, 200, "{body}");
        assert!(
            body["result"]["tools"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v["name"] == "cache_refresh")
        );
    }
    let (_, body) = rpc(&app, Some("synthetic-client-a"), "tools/list", json!({})).await;
    assert!(body["result"]["tools"].is_array());
    assert_eq!(counts.lookup.load(Ordering::SeqCst), 2);
    for (token, expected) in [("synthetic-client-a", "A"), ("synthetic-client-b", "B")] {
        let (status, body) = rpc(
            &app,
            Some(token),
            "tools/call",
            json!({"name":"kv_read","arguments":{"mount":"secret","path":"allowed"}}),
        )
        .await;
        assert_eq!(status, 200, "{body}");
        assert_ne!(body["result"]["isError"], true, "{body}");
        let returned: Value =
            serde_json::from_str(body["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(returned["data"]["data"]["client"], expected);
    }
    let (_, body) = rpc(
        &app,
        Some("synthetic-client-a"),
        "tools/call",
        json!({"name":"kv_read","arguments":{"mount":"secret","path":"denied"}}),
    )
    .await;
    assert_eq!(body["result"]["isError"], true);
    assert!(!body.to_string().contains("synthetic-client-a"));
    let (_, own) = rpc(
        &app,
        Some("synthetic-client-a"),
        "tools/call",
        json!({"name":"kv_read","arguments":{"mount":"secret","path":"allowed"}}),
    )
    .await;
    assert_ne!(
        own["result"]["isError"], true,
        "A denied path must not lock out other allowed paths: {own}"
    );
    assert_eq!(counts.requests.load(Ordering::SeqCst), 3);
    handle.abort();
}
#[tokio::test]
async fn health_is_local_and_credentials_cannot_be_smuggled_in_tool_args() {
    let (_temp, app, counts, handle) = fixture().await;
    let response = app
        .clone()
        .oneshot(
            http::Request::builder()
                .uri("/healthz")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(counts.lookup.load(Ordering::SeqCst), 0);
    let (_,body)=rpc(&app,Some("synthetic-client-b"),"tools/call",json!({"name":"bao_request","arguments":{"operation":"read","path":"secret/data/allowed","token":"synthetic-client-a"}})).await;
    assert_eq!(body["result"]["isError"], true);
    assert_eq!(counts.requests.load(Ordering::SeqCst), 0);
    handle.abort();
}

async fn request_body_cases(cases: &[(usize, bool, bool)], forwarded: usize) {
    let (_temp, app, counts, handle) = fixture().await;
    for &(size, streamed, accepted) in cases {
        let mut message = json!({
            "jsonrpc":"2.0", "id":1, "method":"tools/call",
            "params":{"name":"bao_request","arguments":{
                "operation":"write", "path":"secret/data/allowed",
                "data":{"data":{"payload":""}}
            }}
        });
        let overhead = message.to_string().len();
        message["params"]["arguments"]["data"]["data"]["payload"] =
            Value::String("x".repeat(size - overhead));
        let expected_forwarded = message["params"]["arguments"]["data"].to_string().len();
        let bytes = axum::body::Bytes::from(message.to_string());
        assert_eq!(bytes.len(), size);
        let mut request = http::Request::builder()
            .method("POST")
            .uri("/mcp")
            .header("host", "localhost")
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream")
            .header("mcp-protocol-version", "2025-06-18")
            .header("authorization", "Bearer synthetic-client-a");
        let body = if streamed {
            axum::body::Body::from_stream(tokio_stream::iter([
                Ok::<_, std::io::Error>(bytes.slice(..size / 2)),
                Ok(bytes.slice(size / 2..)),
            ]))
        } else {
            request = request.header("content-length", size.to_string());
            axum::body::Body::from(bytes)
        };
        let response = app
            .clone()
            .oneshot(request.body(body).unwrap())
            .await
            .unwrap();
        if accepted {
            assert_eq!(response.status(), http::StatusCode::OK);
            let bytes = response.into_body().collect().await.unwrap().to_bytes();
            let result: Value = serde_json::from_slice(&bytes).unwrap();
            assert_ne!(result["result"]["isError"], true);
            let returned: Value =
                serde_json::from_str(result["result"]["content"][0]["text"].as_str().unwrap())
                    .unwrap();
            assert_eq!(returned["data"]["received_bytes"], expected_forwarded);
        } else {
            assert_eq!(response.status(), http::StatusCode::PAYLOAD_TOO_LARGE);
        }
    }
    assert_eq!(counts.requests.load(Ordering::SeqCst), forwarded);
    handle.abort();
}

#[tokio::test]
async fn request_body_limit_accepts_eight_mib() {
    request_body_cases(
        &[
            (3 * 1024 * 1024, false, true),
            (8 * 1024 * 1024, true, true),
        ],
        2,
    )
    .await;
}

#[tokio::test]
async fn request_body_limit_rejects_larger_bodies() {
    let size = 8 * 1024 * 1024 + 1;
    request_body_cases(&[(size, false, false), (size, true, false)], 0).await;
}

#[tokio::test]
async fn incomplete_bodies_time_out_and_release_all_request_slots() {
    let (_temp, app, counts, handle) = fixture_with_body_timeout(2).await;
    let (status, _) = rpc(&app, Some("synthetic-client-a"), "tools/list", json!({})).await;
    assert_eq!(status, 200);
    let started = Arc::new(AtomicUsize::new(0));
    let mut pending = Vec::new();
    for i in 0..64 {
        let started = started.clone();
        let first = if i % 2 == 0 { "" } else { "{\"jsonrpc\":" };
        let stream = tokio_stream::iter([Ok::<_, std::io::Error>(axum::body::Bytes::from_static(
            first.as_bytes(),
        ))])
        .map(move |chunk| {
            started.fetch_add(1, Ordering::SeqCst);
            chunk
        })
        .chain(tokio_stream::pending());
        let request = http::Request::builder()
            .method("POST")
            .uri(if i % 2 == 0 { "/mcp" } else { "/mcp/" })
            .header("host", "localhost")
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream")
            .header("authorization", "Bearer synthetic-client-a")
            .body(axum::body::Body::from_stream(stream))
            .unwrap();
        pending.push(tokio::spawn(app.clone().oneshot(request)));
    }
    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        while started.load(Ordering::SeqCst) < 64 {
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
    })
    .await
    .unwrap();
    let (status, _) = rpc(&app, Some("synthetic-client-a"), "tools/list", json!({})).await;
    assert_eq!(status, 429);
    for request in pending {
        assert_eq!(request.await.unwrap().unwrap().status(), 408);
    }
    let (status, _) = rpc(&app, Some("synthetic-client-a"), "tools/list", json!({})).await;
    assert_eq!(status, 200, "Timed-out bodies must release every permit");
    assert_eq!(counts.requests.load(Ordering::SeqCst), 0);
    handle.abort();
}

#[tokio::test]
async fn body_deadline_does_not_cancel_a_dispatched_mutation() {
    let (_temp, app, counts, handle) = fixture_with_body_timeout(1).await;
    let (status, response) = rpc(
        &app,
        Some("synthetic-client-a"),
        "tools/call",
        json!({
            "name":"bao_request", "arguments":{
                "operation":"write", "path":"secret/data/allowed", "data":{"slow":true}
            }
        }),
    )
    .await;
    assert_eq!(status, 200);
    assert_ne!(response["result"]["isError"], true);
    assert_eq!(counts.requests.load(Ordering::SeqCst), 1);
    handle.abort();
}
