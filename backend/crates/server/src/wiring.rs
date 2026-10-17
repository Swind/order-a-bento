//! Builds the application state (adapters + use cases) from configuration.

use std::sync::Arc;

use anyhow::Context;
use api::{AppState, RateLimitConfig, WebSettings};
use application::ai_import::{AiImportConfig, AiImportService, ImportAgent};
use application::ports::IdentityProvider;
use application::{AuthConfig, AuthService, DirectoryService, Maintenance, OrderingService};
use domain::shared::SystemClock;
use infrastructure::ai::{DemoImportAgent, RigAgentConfig, RigImportAgent};
use infrastructure::ai_import_repo::SqliteImportRunRepo;
use infrastructure::directory_repo::{SqliteRosterRepo, SqliteVendorRepo};
use infrastructure::google::GoogleOidcProvider;
use infrastructure::identity_repo::{SqliteOAuthStateRepo, SqliteOrganizerRepo, SqliteSessionRepo};
use infrastructure::maintenance_repo::SqliteMaintenanceRepo;
use infrastructure::ordering_repo::{
    SqliteGroupOrderRepo, SqliteOrderReadModel, SqliteSettlementRepo, SqliteSubmissionRepo,
};
use infrastructure::sqlite::Db;
use infrastructure::tokens::OsRngTokenGenerator;

use crate::config::{AiMode, Config};

/// Google provider when `GOOGLE_CLIENT_ID` and `GOOGLE_CLIENT_SECRET` are both set.
/// No network access happens here (discovery is lazy).
pub fn google_provider(config: &Config) -> anyhow::Result<Option<Arc<dyn IdentityProvider>>> {
    match (&config.google_client_id, &config.google_client_secret) {
        (Some(id), Some(secret)) => {
            let redirect = format!(
                "{}/auth/google/callback",
                config.app_base_url.trim_end_matches('/')
            );
            let p = GoogleOidcProvider::new(id, secret, &redirect)
                .map_err(|e| anyhow::anyhow!("{e}"))
                .context("configuring Google OIDC")?;
            Ok(Some(Arc::new(p)))
        }
        _ => Ok(None),
    }
}

/// The AI import service for the configured mode (`None` = AI disabled), and
/// whether it runs the scripted demo agent.
pub fn ai_import_service(
    config: &Config,
    db: &Db,
) -> anyhow::Result<Option<(AiImportService, bool)>> {
    let (agent, demo): (Arc<dyn ImportAgent>, bool) = match config.ai_mode() {
        AiMode::Disabled => return Ok(None),
        AiMode::Demo => (Arc::new(DemoImportAgent::default()), true),
        AiMode::Anthropic => {
            let key = config
                .anthropic_api_key
                .clone()
                .context("ANTHROPIC_API_KEY")?;
            let mut cfg = RigAgentConfig::new(key, config.ai_model.clone());
            cfg.base_url = config.anthropic_base_url.clone();
            cfg.max_turns = config.ai_max_turns as usize;
            cfg.fallbacks = config.ai_fallbacks;
            let agent = RigImportAgent::new(cfg).map_err(|e| anyhow::anyhow!(e))?;
            (Arc::new(agent), false)
        }
    };
    let runs = Arc::new(SqliteImportRunRepo::new(db.clone()));
    let model = if demo { "demo" } else { &config.ai_model };
    Ok(Some((
        AiImportService {
            runs: runs.clone(),
            apply_repo: runs,
            rosters: Arc::new(SqliteRosterRepo::new(db.clone())),
            vendors: Arc::new(SqliteVendorRepo::new(db.clone())),
            agent,
            clock: Arc::new(SystemClock),
            config: AiImportConfig::new(model, config.ai_daily_quota_per_user),
        },
        demo,
    )))
}

pub fn build_state(config: &Config, db: Db) -> anyhow::Result<AppState> {
    let auth = AuthService {
        organizers: Arc::new(SqliteOrganizerRepo::new(db.clone())),
        sessions: Arc::new(SqliteSessionRepo::new(db.clone())),
        oauth_states: Arc::new(SqliteOAuthStateRepo::new(db.clone())),
        identity: google_provider(config)?,
        tokens: Arc::new(OsRngTokenGenerator),
        clock: Arc::new(SystemClock),
        config: AuthConfig::from_days(config.session_ttl_days),
    };
    let directory = DirectoryService {
        rosters: Arc::new(SqliteRosterRepo::new(db.clone())),
        vendors: Arc::new(SqliteVendorRepo::new(db.clone())),
        clock: Arc::new(SystemClock),
    };
    let rosters = Arc::new(SqliteRosterRepo::new(db.clone()));
    let vendors = Arc::new(SqliteVendorRepo::new(db.clone()));
    let ordering = OrderingService {
        groups: Arc::new(SqliteGroupOrderRepo::new(db.clone())),
        submissions: Arc::new(SqliteSubmissionRepo::new(db.clone())),
        read_model: Arc::new(SqliteOrderReadModel::new(db.clone())),
        management: Arc::new(SqliteOrderReadModel::new(db.clone())),
        settlements: Arc::new(SqliteSettlementRepo::new(db.clone())),
        rosters,
        vendors,
        tokens: Arc::new(OsRngTokenGenerator),
        clock: Arc::new(SystemClock),
        app_base_url: config.app_base_url.clone(),
    };
    let trusted = api::client::parse_trusted_proxies(&config.trusted_proxies)
        .map_err(|e| anyhow::anyhow!(e))?;
    let web = WebSettings::new(
        &config.app_base_url,
        &config.allowed_origins,
        config.dev_login,
    )
    .with_trusted_proxies(trusted)
    .with_display_offset(config.display_utc_offset);
    let ai = ai_import_service(config, &db)?;
    let mut state = AppState::new(db, !config.is_production(), auth, directory, ordering, web)
        .with_rate_limit(RateLimitConfig {
            burst: config.public_rate_limit_burst,
            per_sec: config.public_rate_limit_per_sec,
        });
    if let Some((service, demo)) = ai {
        state = state.with_ai_import(service, demo);
    }
    Ok(state)
}

/// The background housekeeping use case (design §10/§11).
pub fn maintenance(config: &Config, db: &Db) -> Maintenance {
    Maintenance {
        sessions: Arc::new(SqliteSessionRepo::new(db.clone())),
        oauth_states: Arc::new(SqliteOAuthStateRepo::new(db.clone())),
        repo: Arc::new(SqliteMaintenanceRepo::new(db.clone())),
        clock: Arc::new(SystemClock),
        client_info_retention_days: config.client_info_retention_days,
    }
}
