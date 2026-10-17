use server::config::{AiMode, Config};

use std::net::SocketAddr;

use anyhow::Context;
use infrastructure::sqlite::Db;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::from_env().context("loading config")?;

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,tower_http=info")),
        )
        .init();

    let db = Db::connect(&config.database_url)
        .await
        .context("connecting to database")?;
    db.migrate().await.context("running migrations")?;

    if config.dev_login {
        tracing::warn!(
            "DEV_LOGIN is enabled: POST /auth/dev-login is available (development only)"
        );
    }
    if config.google_client_id.is_none() || config.google_client_secret.is_none() {
        tracing::warn!("Google login is not configured; /auth/google/* will return 503");
    }
    match config.ai_mode() {
        AiMode::Anthropic => tracing::info!(model = %config.ai_model, "AI import: Anthropic"),
        AiMode::Demo => tracing::warn!("AI import runs on scripted DEMO data (AI_DEMO=true)"),
        AiMode::Disabled => {
            tracing::info!(
                "AI import is disabled (set ANTHROPIC_API_KEY, or AI_DEMO=true in development)"
            )
        }
    }
    let maintenance = server::wiring::maintenance(&config, &db);
    let app = api::router(server::wiring::build_state(&config, db)?);
    let addr = SocketAddr::from(([0, 0, 0, 0], config.port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("binding {addr}"))?;
    tracing::info!(%addr, env = ?config.app_env, "listening");

    let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
    let job = tokio::spawn(maintenance_loop(
        maintenance,
        std::time::Duration::from_secs(u64::from(config.maintenance_interval_hours) * 3600),
        stop_rx,
    ));
    let served = axum_serve(listener, app).await;
    let _ = stop_tx.send(true);
    let _ = job.await;
    served
}

/// Run housekeeping shortly after startup, then every `interval`, until `stop` fires.
/// Failures are logged and never end the loop.
async fn maintenance_loop(
    m: application::Maintenance,
    interval: std::time::Duration,
    mut stop: tokio::sync::watch::Receiver<bool>,
) {
    let mut delay = std::time::Duration::from_secs(30);
    loop {
        tokio::select! {
            _ = tokio::time::sleep(delay) => {}
            _ = stop.changed() => return,
        }
        delay = interval;
        let (r, err) = m.run_once().await;
        match err {
            None => tracing::info!(
                sessions = r.sessions_deleted,
                oauth_states = r.oauth_states_deleted,
                client_info_cleared = r.client_info_cleared,
                import_runs_failed = r.import_runs_failed,
                "maintenance done"
            ),
            Some(e) => tracing::warn!(error = %e, ?r, "maintenance finished with errors"),
        }
    }
}

async fn axum_serve(listener: tokio::net::TcpListener, app: axum::Router) -> anyhow::Result<()> {
    // `ConnectInfo<SocketAddr>` provides the TCP peer address (client IP).
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await
    .context("serving")
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let term = async {
        if let Ok(mut s) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            s.recv().await;
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();
    tokio::select! { _ = ctrl_c => {}, _ = term => {} }
    tracing::info!("shutting down");
}
