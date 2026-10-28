use crate::{
    backend::{Backend, Credential},
    cache::Cache,
    catalog,
    config::Config,
    discovery::Discovery,
    error::{Error, Result},
    operations,
};
use axum::{
    Json, Router,
    extract::{Request, State},
    middleware::{self, Next},
    response::{IntoResponse, Response},
};
use rmcp::{
    ErrorData as McpError, RoleServer, ServerHandler,
    model::*,
    service::RequestContext,
    transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
    },
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{borrow::Cow, sync::Arc, time::Instant};
use tokio::sync::Semaphore;
#[derive(Clone)]
pub struct App {
    pub backend: Backend,
    pub discovery: Discovery,
    requests: Arc<Semaphore>,
}
impl App {
    pub fn new(config: &Config) -> Result<Self> {
        let backend = Backend::new(
            &config.addr,
            &config.namespace,
            config.ca_file.as_deref(),
            config.timeout,
        )?;
        let cache = Cache::open(
            &config.cache_path,
            config.cache_idle_ttl,
            config.cache_max_age,
            config.cache_max_entries,
        )?;
        let discovery = Discovery::new(backend.clone(), cache);
        Ok(Self {
            backend,
            discovery,
            requests: Arc::new(Semaphore::new(64)),
        })
    }
}
fn failure(error: Error) -> Response {
    (
        http::StatusCode::from_u16(error.status).unwrap_or(http::StatusCode::INTERNAL_SERVER_ERROR),
        Json(json!({"error":error})),
    )
        .into_response()
}
async fn authenticate(State(app): State<App>, mut request: Request, next: Next) -> Response {
    let Ok(_permit) = app.requests.clone().try_acquire_owned() else {
        return failure(Error::new(
            429,
            "rate_limited",
            "Too many concurrent requests",
        ));
    };
    let values = request.headers().get_all(http::header::AUTHORIZATION);
    let mut values = values.iter();
    let Some(header) = values.next() else {
        return failure(Error::unauthorized());
    };
    if values.next().is_some() {
        return failure(Error::unauthorized());
    }
    let Some(token) = header.to_str().ok().and_then(|s| s.strip_prefix("Bearer ")) else {
        return failure(Error::unauthorized());
    };
    let credential = match Credential::new(token) {
        Ok(v) => v,
        Err(_) => return failure(Error::unauthorized()),
    };
    // Authentication/discovery is short-lived cached; execution always uses Bao's live ACL.
    if let Err(e) = app.discovery.get(&credential, false).await {
        tracing::warn!(actor=%credential.hash(),code=%e.code,status=e.status,"request_denied");
        return failure(e);
    }
    request.extensions_mut().insert(credential);
    next.run(request).await
}
fn identity(ctx: &RequestContext<RoleServer>) -> std::result::Result<Credential, McpError> {
    ctx.extensions
        .get::<http::request::Parts>()
        .and_then(|p| p.extensions.get::<Credential>())
        .cloned()
        .or_else(|| ctx.extensions.get::<Credential>().cloned())
        .ok_or_else(|| McpError::invalid_request("Authentication context is missing", None))
}
#[derive(Clone)]
struct McpServer {
    app: App,
}
impl ServerHandler for McpServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions("Thin OpenBao gateway. Use native OpenBao tokens and API data. Listed tool families are discovery hints, not a grant for every argument. OpenBao authorizes each execution. cache_refresh refreshes only your token's discovery. No namespace or engine is initialized at startup.")
    }
    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        ctx: RequestContext<RoleServer>,
    ) -> std::result::Result<ListToolsResult, McpError> {
        let credential = identity(&ctx)?;
        let snapshot = self
            .app
            .discovery
            .get(&credential, false)
            .await
            .map_err(|e| {
                McpError::internal_error(e.message, Some(json!({"code":e.code,"status":e.status})))
            })?;
        let tools = catalog::all()
            .into_iter()
            .filter(|d| snapshot.tools.contains(&d.name))
            .map(|d| {
                let mut tool = Tool::default();
                tool.name = Cow::Owned(d.name);
                tool.description = Some(Cow::Owned(d.description));
                tool.input_schema =
                    Arc::new(d.input_schema.as_object().cloned().unwrap_or_default());
                tool
            })
            .collect();
        tracing::info!(actor=%credential.hash(),tool="tools/list",outcome="success","mcp_operation");
        Ok(ListToolsResult::with_all_items(tools))
    }
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        ctx: RequestContext<RoleServer>,
    ) -> std::result::Result<CallToolResponse, McpError> {
        let credential = identity(&ctx)?;
        let name = request.name.as_ref();
        if catalog::get(name).is_none() {
            return Err(McpError::new(
                ErrorCode::METHOD_NOT_FOUND,
                "Unknown tool",
                None,
            ));
        }
        let args = Value::Object(request.arguments.unwrap_or_default());
        let resource = ["mount", "vault_id", "path"]
            .map(|key| args.get(key).and_then(Value::as_str).unwrap_or_default())
            .join("/");
        let resource_hash = hex::encode(Sha256::digest(resource.as_bytes()));
        let start = Instant::now();
        let result = if name == "cache_refresh" {
            if args.as_object().is_some_and(|a| !a.is_empty()) {
                Err(Error::invalid("cache_refresh accepts no arguments"))
            } else {
                self.app
                    .discovery
                    .get(&credential, true)
                    .await
                    .and_then(|s| serde_json::to_value(s).map_err(|_| Error::internal()))
            }
        } else {
            operations::execute(&self.app.backend, &credential, name, args).await
        };
        match result {
            Ok(value) => {
                let partial =
                    name == "vault_create" && value.get("complete") == Some(&Value::Bool(false));
                if partial
                    && value
                        .pointer("/error/status")
                        .and_then(Value::as_u64)
                        .is_some_and(|s| s == 401 || s == 403)
                {
                    let _ = self.app.discovery.invalidate(&credential).await;
                }
                tracing::info!(actor=%credential.hash(),tool=name,resource=%resource_hash,outcome=if partial {"partial"} else {"success"},elapsed_ms=start.elapsed().as_millis() as u64,"mcp_operation");
                let content = vec![ContentBlock::text(value.to_string())];
                Ok(if partial {
                    CallToolResult::error(content)
                } else {
                    CallToolResult::success(content)
                }
                .into())
            }
            Err(e) => {
                if e.status == 401 || e.status == 403 {
                    let _ = self.app.discovery.invalidate(&credential).await;
                }
                tracing::warn!(actor=%credential.hash(),tool=name,resource=%resource_hash,code=%e.code,status=e.status,elapsed_ms=start.elapsed().as_millis() as u64,"mcp_operation_failed");
                Ok(
                    CallToolResult::error(vec![ContentBlock::text(json!({"error":e}).to_string())])
                        .into(),
                )
            }
        }
    }
}
pub fn router(app: App, config: &Config) -> Router {
    let server_app = app.clone();
    let transport = StreamableHttpServerConfig::default()
        .with_legacy_session_mode(false)
        .with_json_response(true)
        .with_allowed_hosts(config.allowed_hosts.clone())
        .with_allowed_origins(config.allowed_origins.clone())
        .with_max_request_body_bytes(2 * 1024 * 1024);
    let service = StreamableHttpService::new(
        move || {
            Ok(McpServer {
                app: server_app.clone(),
            })
        },
        Arc::new(LocalSessionManager::default()),
        transport,
    );
    Router::new()
        .route_service("/mcp", service.clone())
        .route_service("/mcp/", service)
        .layer(middleware::from_fn_with_state(app, authenticate))
        .layer(tower_http::limit::RequestBodyLimitLayer::new(
            2 * 1024 * 1024,
        ))
        .route(
            "/healthz",
            axum::routing::get(|| async { Json(json!({"status":"ok"})) }),
        )
}
