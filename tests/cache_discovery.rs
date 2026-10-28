use axum::{
    Router,
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::any,
};
use bao_mcp::{
    backend::{Backend, Credential},
    cache::Cache,
    discovery::Discovery,
};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

#[derive(Clone, Default)]
struct Stub {
    requests: Arc<Mutex<Vec<(String, String)>>>,
    revoked: Arc<AtomicBool>,
}

async fn upstream(
    State(stub): State<Stub>,
    Path(path): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> axum::response::Response {
    let token = headers
        .get("X-Vault-Token")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    stub.requests
        .lock()
        .unwrap()
        .push((token.to_owned(), path.clone()));
    if token == "invalid-token" || stub.revoked.load(Ordering::SeqCst) {
        return (
            StatusCode::FORBIDDEN,
            axum::Json(json!({"errors":["permission denied"]})),
        )
            .into_response();
    }
    assert_eq!(headers.get("X-Vault-Namespace").unwrap(), "agent");
    let value = match path.as_str() {
        "auth/token/lookup-self" => {
            json!({"data":{"id":token,"accessor":"never-persist-accessor","ttl":if token=="short-token" {5} else {3600},"metadata":{"password":"never-persist-identity"}}})
        }
        "sys/internal/ui/mounts" => json!({"data":{"secret":{
            "team/":{"type":"kv","options":{"version":"2","password":"never-persist-options"},"description":"never-persist-description"},
            "public-transit/":{"type":"transit","listing_visibility":"unauth"}
        },"auth":{}}}),
        "sys/internal/ui/resultant-acl" => {
            if token == "plus-token" {
                // Pinned Bao omits ACL '+' rules. There is no concrete witness to enumerate here.
                json!({"data":{"root":false,"exact_paths":{},"glob_paths":{}}})
            } else {
                json!({"data":{"root":false,"exact_paths":{
                "agent/team/data/apps/api":{"capabilities":["read"],"allowed_parameters":{"password":["never-persist-parameter"]}},
                "other/team/data/forbidden":{"capabilities":["update"]}
            },"glob_paths":{}}})
            }
        }
        "sys/capabilities-self" => {
            let input: Value = serde_json::from_slice(&body).unwrap();
            let mut paths = serde_json::Map::new();
            for path in input["paths"].as_array().unwrap() {
                let path = path.as_str().unwrap();
                paths.insert(
                    path.into(),
                    if path == "team/data/apps/api" && token != "plus-token" {
                        json!(["read"])
                    } else {
                        json!(["deny"])
                    },
                );
            }
            json!({"data":paths})
        }
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    axum::Json(value).into_response()
}

async fn setup() -> (
    tempfile::TempDir,
    Discovery,
    Cache,
    Stub,
    tokio::task::JoinHandle<()>,
) {
    let stub = Stub::default();
    let app = Router::new()
        .route("/v1/{*path}", any(upstream))
        .with_state(stub.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let dir = tempfile::tempdir().unwrap();
    let cache = Cache::open(
        &dir.path().join("cache.db"),
        Duration::from_secs(3600),
        Duration::from_secs(60),
        16,
    )
    .unwrap();
    let backend = Backend::new(
        &format!("http://{addr}"),
        "agent",
        None,
        false,
        Duration::from_secs(2),
    )
    .unwrap();
    let discovery = Discovery::new(backend, cache.clone());
    (dir, discovery, cache, stub, server)
}

#[tokio::test]
async fn narrow_read_grant_is_discovered_without_exposing_write_or_caching_sensitive_fields() {
    let (dir, discovery, _cache, stub, server) = setup().await;
    let credential = Credential::new("read-token").unwrap();
    let snapshot = discovery.get(&credential, false).await.unwrap();
    assert!(snapshot.tools.iter().any(|tool| tool == "kv_read"));
    for absent in [
        "kv_write",
        "kv_manage",
        "vault_create",
        "transit_keys",
        "transit_crypto",
    ] {
        assert!(
            !snapshot.tools.iter().any(|tool| tool == absent),
            "unexpected tool {absent}"
        );
    }
    assert_eq!(
        snapshot.capabilities["paths"]["team/data/apps/api"],
        json!(["read"])
    );
    assert!(
        snapshot.capabilities["acl_patterns"]
            .get("other/team/data/forbidden")
            .is_none()
    );
    assert!(!snapshot.complete);
    assert!(snapshot.expires_at <= snapshot.fetched_at + 60);
    let before = stub.requests.lock().unwrap().len();
    discovery.get(&credential, false).await.unwrap();
    assert_eq!(
        stub.requests.lock().unwrap().len(),
        before,
        "cache hits must not spend token uses on metadata calls"
    );
    let bytes = std::fs::read(dir.path().join("cache.db")).unwrap();
    let contents = String::from_utf8_lossy(&bytes);
    for sensitive in [
        "read-token",
        "never-persist-accessor",
        "never-persist-identity",
        "never-persist-options",
        "never-persist-description",
        "never-persist-parameter",
    ] {
        assert!(!contents.contains(sensitive), "persisted {sensitive}");
    }
    server.abort();
}

#[tokio::test]
async fn omitted_plus_rules_keep_generic_fallback_and_do_not_invent_permissions() {
    let (_dir, discovery, _cache, _stub, server) = setup().await;
    let snapshot = discovery
        .get(&Credential::new("plus-token").unwrap(), false)
        .await
        .unwrap();
    for name in ["bao_request", "api_help", "cache_refresh"] {
        assert!(snapshot.tools.iter().any(|tool| tool == name));
    }
    assert!(!snapshot.tools.iter().any(|tool| tool == "kv_write"));
    assert!(!snapshot.complete);
    assert!(
        snapshot
            .warnings
            .iter()
            .any(|warning| warning.contains("'+'"))
    );
    assert_eq!(snapshot.capabilities["authoritative_for_execution"], false);
    server.abort();
}

#[tokio::test]
async fn invalid_tokens_are_not_persisted_and_are_throttled() {
    let (dir, discovery, _cache, stub, server) = setup().await;
    let credential = Credential::new("invalid-token").unwrap();
    assert_eq!(
        discovery.get(&credential, false).await.unwrap_err().status,
        403
    );
    assert_eq!(
        discovery.get(&credential, true).await.unwrap_err().status,
        429
    );
    assert_eq!(stub.requests.lock().unwrap().len(), 1);
    let db = rusqlite::Connection::open(dir.path().join("cache.db")).unwrap();
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM discovery", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    server.abort();
}

#[tokio::test]
async fn token_ttl_bounds_snapshot_and_scope_isolation_prevents_reuse() {
    let (_dir, discovery, cache, stub, server) = setup().await;
    let snapshot = discovery
        .get(&Credential::new("short-token").unwrap(), false)
        .await
        .unwrap();
    assert!(snapshot.expires_at <= snapshot.fetched_at + 5);
    let credential = Credential::new("read-token").unwrap();
    discovery.get(&credential, false).await.unwrap();
    // Same credential and DB, different backend scope: it must authenticate again.
    let other = Discovery::new(
        Backend::new(
            "http://127.0.0.1:1",
            "agent",
            None,
            false,
            Duration::from_millis(100),
        )
        .unwrap(),
        cache,
    );
    assert!(other.get(&credential, false).await.is_err());
    assert!(
        stub.requests
            .lock()
            .unwrap()
            .iter()
            .any(|(_, path)| path == "auth/token/lookup-self")
    );
    server.abort();
}

#[tokio::test]
async fn invalidate_removes_only_current_scoped_entry_and_refresh_preserves_snapshot_until_validation()
 {
    let (dir, discovery, _cache, _stub, server) = setup().await;
    let first = Credential::new("read-token").unwrap();
    let second = Credential::new("another-token").unwrap();
    discovery.get(&first, false).await.unwrap();
    discovery.get(&second, false).await.unwrap();
    // A refresh within the minimum interval rejects without deleting the valid entry.
    assert_eq!(discovery.get(&first, true).await.unwrap_err().status, 429);
    discovery.get(&first, false).await.unwrap();
    discovery.invalidate(&first).await.unwrap();
    let db = rusqlite::Connection::open(dir.path().join("cache.db")).unwrap();
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM discovery", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        db.query_row("SELECT token_hash FROM discovery", [], |row| row
            .get::<_, String>(0))
            .unwrap(),
        second.hash()
    );
    server.abort();
}
