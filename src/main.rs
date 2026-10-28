use bao_mcp::{
    config::Config,
    server::{App, router},
};
#[tokio::main]
async fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!(
            "bao-mcp [serve]\n\nRequired: BAO_ADDR\nDefaults: BAO_NAMESPACE=agent MCP_LISTEN=127.0.0.1:8000 CACHE_PATH=cache.db\nExpose OpenBao as MCP tools. Send Authorization: Bearer <OpenBao token> to /mcp.\nPrepare the namespace with scripts/setup-bao-environment.sh."
        );
        return;
    }
    if !args.is_empty() && args != ["serve"] {
        eprintln!("Only the serve command is supported");
        std::process::exit(2);
    }
    // Restrict logging to this application; never enable HTTP wire/body/header traces.
    let level = match std::env::var("LOG_LEVEL").as_deref() {
        Ok("error") => "error",
        Ok("warn") => "warn",
        _ => "info",
    };
    tracing_subscriber::fmt()
        .with_env_filter(format!("bao_mcp={level}"))
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();
    if let Err(error) = run().await {
        tracing::error!(code=%error.code,"startup_failed");
        eprintln!("{error}");
        std::process::exit(1);
    }
}
async fn run() -> bao_mcp::Result<()> {
    let cfg = Config::from_env()?;
    let app = App::new(&cfg)?;
    let listener = tokio::net::TcpListener::bind(cfg.bind)
        .await
        .map_err(|_| bao_mcp::Error::new(500, "listen_failed", "Cannot bind MCP_LISTEN"))?;
    tracing::info!(listen=%cfg.bind,"bao_mcp_started");
    axum::serve(listener, router(app, &cfg))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .map_err(|_| bao_mcp::Error::internal())
}
