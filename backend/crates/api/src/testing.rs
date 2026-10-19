//! Test helpers: a fully wired `AppState` on a temp SQLite DB with a fake Google provider.

use std::sync::Arc;

use application::ai_import::{AiImportConfig, AiImportService, ImportAgent};
use application::ports::{IdentityError, IdentityProvider, VerifiedIdentity};
use application::{AuthConfig, AuthService, DirectoryService, OrderingService};
use async_trait::async_trait;
use domain::shared::SystemClock;
use infrastructure::ai_import_repo::SqliteImportRunRepo;
use infrastructure::directory_repo::{SqliteRosterRepo, SqliteVendorRepo};
use infrastructure::identity_repo::{SqliteOAuthStateRepo, SqliteOrganizerRepo, SqliteSessionRepo};
use infrastructure::ordering_repo::{
    SqliteGroupOrderRepo, SqliteOrderReadModel, SqliteSettlementRepo, SqliteSubmissionRepo,
};
use infrastructure::password::Argon2PasswordHasher;
use infrastructure::sqlite::{Db, TempDir};
use infrastructure::tokens::OsRngTokenGenerator;

use crate::{AppState, RateLimitConfig, WebSettings};
use ipnet::IpNet;

pub struct FakeIdp;

#[async_trait]
impl IdentityProvider for FakeIdp {
    async fn authorize_url(
        &self,
        state: &str,
        nonce: &str,
        challenge: &str,
    ) -> Result<String, IdentityError> {
        Ok(format!(
            "https://accounts.fake/auth?state={state}&nonce={nonce}&code_challenge={challenge}"
        ))
    }

    async fn exchange(
        &self,
        code: &str,
        _verifier: &str,
        _nonce: &str,
    ) -> Result<VerifiedIdentity, IdentityError> {
        match code {
            "good" => Ok(VerifiedIdentity {
                sub: "g-123".into(),
                email: "google@user.test".into(),
                email_verified: true,
                name: Some("Gina Google".into()),
                picture: Some("https://pics.test/g.png".into()),
            }),
            "unverified" => Ok(VerifiedIdentity {
                sub: "g-456".into(),
                email: "u@user.test".into(),
                email_verified: false,
                name: None,
                picture: None,
            }),
            _ => Err(IdentityError::Provider("bad code".into())),
        }
    }
}

pub struct TestApp {
    pub state: AppState,
    pub _dir: TempDir,
}

pub async fn test_app(dev_login: bool, with_google: bool) -> TestApp {
    // Effectively unlimited so that functional tests are never rate limited.
    let rate = RateLimitConfig {
        burst: 1_000_000,
        per_sec: 1_000_000.0,
    };
    test_app_with(dev_login, with_google, vec![], rate).await
}

pub async fn test_app_with(
    dev_login: bool,
    with_google: bool,
    trusted_proxies: Vec<IpNet>,
    rate: RateLimitConfig,
) -> TestApp {
    let (db, dir) = Db::open_temp().await.unwrap();
    let identity: Option<Arc<dyn IdentityProvider>> = with_google.then(|| Arc::new(FakeIdp) as _);
    let auth = AuthService {
        organizers: Arc::new(SqliteOrganizerRepo::new(db.clone())),
        sessions: Arc::new(SqliteSessionRepo::new(db.clone())),
        oauth_states: Arc::new(SqliteOAuthStateRepo::new(db.clone())),
        identity,
        hasher: Arc::new(Argon2PasswordHasher::new()),
        tokens: Arc::new(OsRngTokenGenerator),
        clock: Arc::new(SystemClock),
        config: AuthConfig::from_days(14),
    };
    let directory = DirectoryService {
        delivery_addresses: Arc::new(
            infrastructure::delivery_address_repo::SqliteDeliveryAddressRepo::new(db.clone()),
        ),
        rosters: Arc::new(SqliteRosterRepo::new(db.clone())),
        vendors: Arc::new(SqliteVendorRepo::new(db.clone())),
        clock: Arc::new(SystemClock),
    };
    let ordering = OrderingService {
        delivery_addresses: directory.delivery_addresses.clone(),
        groups: Arc::new(SqliteGroupOrderRepo::new(db.clone())),
        submissions: Arc::new(SqliteSubmissionRepo::new(db.clone())),
        read_model: Arc::new(SqliteOrderReadModel::new(db.clone())),
        management: Arc::new(SqliteOrderReadModel::new(db.clone())),
        settlements: Arc::new(SqliteSettlementRepo::new(db.clone())),
        rosters: Arc::new(SqliteRosterRepo::new(db.clone())),
        vendors: Arc::new(SqliteVendorRepo::new(db.clone())),
        tokens: Arc::new(OsRngTokenGenerator),
        clock: Arc::new(SystemClock),
        app_base_url: "http://localhost:3000".into(),
    };
    let web = WebSettings::new(
        "http://localhost:3000",
        &["http://localhost:5173".to_string()],
        dev_login,
    )
    .with_password_login(true)
    .with_trusted_proxies(trusted_proxies);
    TestApp {
        state: AppState::new(db, true, auth, directory, ordering, web)
            .with_rate_limit(rate)
            .with_login_rate_limit(rate),
        _dir: dir,
    }
}

/// Like [`test_app`] with AI import enabled using `agent`.
/// The per-organizer AI rate limit is `rate` (default tests use a huge one).
pub async fn test_app_ai(
    agent: Arc<dyn ImportAgent>,
    daily_quota: u32,
    rate: Option<RateLimitConfig>,
    demo: bool,
) -> TestApp {
    let mut t = test_app(true, false).await;
    let db = t.state.db.clone();
    let runs = Arc::new(SqliteImportRunRepo::new(db.clone()));
    let mut config = AiImportConfig::new("test-model", daily_quota);
    config.retry_delay = std::time::Duration::from_millis(5);
    let service = AiImportService {
        runs: runs.clone(),
        apply_repo: runs,
        rosters: Arc::new(SqliteRosterRepo::new(db.clone())),
        vendors: Arc::new(SqliteVendorRepo::new(db)),
        agent,
        clock: Arc::new(SystemClock),
        config,
    };
    let rate = rate.unwrap_or(RateLimitConfig {
        burst: 1_000_000,
        per_sec: 1_000_000.0,
    });
    t.state = t
        .state
        .clone()
        .with_ai_import(service, demo)
        .with_ai_rate_limit(rate);
    t
}
