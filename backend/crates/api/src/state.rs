use std::sync::Arc;

use application::ports::UaParser;
use application::{AiImportService, AuthService, DirectoryService, OrderingService};
use infrastructure::sqlite::Db;
use infrastructure::ua::WootheeUaParser;
use ipnet::IpNet;
use time::UtcOffset;

use crate::ratelimit::{KeyedLimiter, RateLimitConfig, RateLimiter};

/// Request-time settings derived from configuration.
#[derive(Debug, Clone)]
pub struct WebSettings {
    /// Whether to add `Secure` to the session cookie (`APP_BASE_URL` is https).
    pub cookie_secure: bool,
    /// Origins (`scheme://host[:port]`) allowed on state-changing requests.
    pub allowed_origins: Vec<String>,
    /// Whether `POST /auth/dev-login` is registered.
    pub dev_login: bool,
    /// Reverse proxies whose `X-Forwarded-For` is trusted (empty = trust none).
    pub trusted_proxies: Vec<IpNet>,
    /// Timezone used when rendering deadlines in link previews (`DISPLAY_UTC_OFFSET`).
    pub display_offset: UtcOffset,
}

impl WebSettings {
    /// Build from `APP_BASE_URL`, extra allowed origins and the dev-login flag.
    pub fn new(app_base_url: &str, extra_origins: &[String], dev_login: bool) -> Self {
        let mut allowed_origins: Vec<String> = Vec::new();
        for o in std::iter::once(app_base_url).chain(extra_origins.iter().map(String::as_str)) {
            let o = origin_of(o);
            if !allowed_origins.contains(&o) {
                allowed_origins.push(o);
            }
        }
        Self {
            cookie_secure: app_base_url.trim().starts_with("https://"),
            allowed_origins,
            dev_login,
            trusted_proxies: Vec::new(),
            display_offset: UtcOffset::from_hms(8, 0, 0).expect("valid offset"),
        }
    }

    pub fn with_display_offset(mut self, offset: UtcOffset) -> Self {
        self.display_offset = offset;
        self
    }

    pub fn with_trusted_proxies(mut self, trusted_proxies: Vec<IpNet>) -> Self {
        self.trusted_proxies = trusted_proxies;
        self
    }
}

/// `scheme://authority` of a URL (no path, no trailing slash, lowercased).
pub fn origin_of(url: &str) -> String {
    let url = url.trim();
    let end = url
        .find("://")
        .map(|i| i + 3)
        .and_then(|start| url[start..].find('/').map(|j| start + j))
        .unwrap_or(url.len());
    url[..end].to_ascii_lowercase()
}

/// Shared application state injected into handlers.
#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    /// Whether `/api/docs` (Scalar UI) is served (false in production).
    pub docs_enabled: bool,
    pub auth: Arc<AuthService>,
    pub directory: Arc<DirectoryService>,
    pub ordering: Arc<OrderingService>,
    pub web: Arc<WebSettings>,
    pub ua_parser: Arc<dyn UaParser>,
    /// Per-client-IP limiter for the public endpoints.
    pub limiter: Arc<RateLimiter>,
    /// AI import use cases; `None` = AI is not configured (routes answer 503).
    pub ai_import: Option<Arc<AiImportService>>,
    /// Whether the AI agent is the scripted demo (the UI shows a badge).
    pub ai_demo: bool,
    /// Per-organizer limiter for starting AI imports (3 per minute).
    pub ai_limiter: Arc<KeyedLimiter<String>>,
}

/// Burst and refill of the per-organizer AI import limiter: 3 requests per minute.
pub const AI_IMPORT_RATE: RateLimitConfig = RateLimitConfig {
    burst: 3,
    per_sec: 3.0 / 60.0,
};

impl AppState {
    pub fn new(
        db: Db,
        docs_enabled: bool,
        auth: AuthService,
        directory: DirectoryService,
        ordering: OrderingService,
        web: WebSettings,
    ) -> Self {
        Self {
            db,
            docs_enabled,
            auth: Arc::new(auth),
            directory: Arc::new(directory),
            ordering: Arc::new(ordering),
            web: Arc::new(web),
            ua_parser: Arc::new(WootheeUaParser),
            limiter: Arc::new(RateLimiter::new(RateLimitConfig::default())),
            ai_import: None,
            ai_demo: false,
            ai_limiter: Arc::new(KeyedLimiter::new(AI_IMPORT_RATE)),
        }
    }

    /// Enable the AI import routes with `service`; `demo` marks the scripted agent.
    pub fn with_ai_import(mut self, service: AiImportService, demo: bool) -> Self {
        self.ai_import = Some(Arc::new(service));
        self.ai_demo = demo;
        self
    }

    /// Replace the per-organizer AI import rate limit (default 3 per minute).
    pub fn with_ai_rate_limit(mut self, cfg: RateLimitConfig) -> Self {
        self.ai_limiter = Arc::new(KeyedLimiter::new(cfg));
        self
    }

    /// Replace the public-endpoint rate limit (default: burst 30, 2 requests/s).
    pub fn with_rate_limit(mut self, cfg: RateLimitConfig) -> Self {
        self.limiter = Arc::new(RateLimiter::new(cfg));
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_normalisation() {
        assert_eq!(origin_of("http://localhost:3000/"), "http://localhost:3000");
        assert_eq!(
            origin_of("https://Bento.Example.com/a/b"),
            "https://bento.example.com"
        );
        assert_eq!(origin_of("http://localhost:5173"), "http://localhost:5173");
    }

    #[test]
    fn settings() {
        let s = WebSettings::new(
            "https://bento.example.com/",
            &[
                "http://localhost:5173".into(),
                "https://bento.example.com".into(),
            ],
            false,
        );
        assert!(s.cookie_secure);
        assert_eq!(
            s.allowed_origins,
            ["https://bento.example.com", "http://localhost:5173"]
        );
        assert!(!WebSettings::new("http://localhost:3000", &[], true).cookie_secure);
    }
}
