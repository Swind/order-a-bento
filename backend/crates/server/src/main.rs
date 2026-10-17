use server::cli::{self, Cli, Command};
use server::config::{AiMode, Config};

use std::net::SocketAddr;

use anyhow::Context;
use clap::Parser;
use infrastructure::sqlite::Db;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let config = Config::from_env().context("loading config")?;
    if let Some(Command::Admin(cmd)) = cli.command {
        return run_admin(&config, cmd).await;
    }

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

    match server::wiring::bootstrap_admin(&config, &db).await? {
        None => {}
        Some(application::BootstrapOutcome::Created) => {
            tracing::info!("created local admin account from ADMIN_EMAIL/ADMIN_PASSWORD")
        }
        Some(application::BootstrapOutcome::AlreadyExists) => tracing::info!(
            "ADMIN_EMAIL already has an account; leaving it (and its password) unchanged"
        ),
    }
    if config.dev_login {
        tracing::warn!(
            "DEV_LOGIN is enabled: POST /auth/dev-login is available (development only)"
        );
    }
    if !config.google_oauth_enabled {
        tracing::warn!(
            "Google login is disabled (GOOGLE_OAUTH_ENABLED=false or no credentials); /auth/google/* will return 503"
        );
    }
    if config.admin_email.is_some() && !config.password_login_enabled {
        tracing::warn!("ADMIN_EMAIL is set but PASSWORD_LOGIN_ENABLED=false: nobody can use it");
    }
    if !config.password_login_enabled {
        tracing::info!("password login is disabled (PASSWORD_LOGIN_ENABLED=false)");
    }
    match config.ai_mode() {
        AiMode::Live => tracing::info!(
            model = %config.ai_model,
            provider = ?config.ai_provider,
            base_url = config.ai_base_url.as_deref().unwrap_or("(provider default)"),
            "AI import: live model"
        ),
        AiMode::Demo => tracing::warn!("AI import runs on scripted DEMO data (AI_DEMO=true)"),
        AiMode::Disabled => {
            tracing::info!("AI import is disabled (set AI_API_KEY, or AI_DEMO=true in development)")
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

/// `server admin ...`: connect, migrate, run the subcommand; exits with its code on failure.
async fn run_admin(config: &Config, cmd: cli::AdminCommand) -> anyhow::Result<()> {
    let db = Db::connect(&config.database_url)
        .await
        .context("connecting to database")?;
    db.migrate().await.context("running migrations")?;
    let svc = server::wiring::account_service(&db);
    let mut source = match &cmd {
        cli::AdminCommand::Create { password_stdin, .. }
        | cli::AdminCommand::SetPassword { password_stdin, .. } => {
            cli::password_source(*password_stdin)
        }
        cli::AdminCommand::List => cli::password_source(true),
    };
    if let Err(e) = cli::run_admin(&svc, &cmd, source.as_mut(), &mut std::io::stdout()).await {
        eprintln!("error: {e}");
        std::process::exit(e.exit_code());
    }
    Ok(())
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
