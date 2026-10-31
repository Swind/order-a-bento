//! Environment-driven configuration (design §11).

use std::collections::HashMap;

use infrastructure::ai::{AiProvider, AiReasoning, DEFAULT_MAX_TOKENS};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppEnv {
    Development,
    Production,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiMode {
    /// A real model behind `AI_PROVIDER`.
    Live,
    Demo,
    Disabled,
}

/// A secret that never shows up in `Debug` output (logs, panics).
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(<redacted>)")
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("invalid value for {key}: {value:?} ({reason})")]
    Invalid {
        key: &'static str,
        value: String,
        reason: &'static str,
    },
}

#[derive(Debug, Clone)]
pub struct Config {
    pub port: u16,
    pub app_env: AppEnv,
    pub dev_login: bool,
    pub app_base_url: String,
    pub database_url: String,
    pub oidc_issuer_url: Option<String>,
    pub oidc_client_id: Option<String>,
    pub oidc_client_secret: Option<Secret>,
    /// Exact extra ID-token audiences trusted in addition to the mandatory client ID.
    pub oidc_additional_audiences: Vec<String>,
    pub oidc_enabled: bool,
    /// `PASSWORD_LOGIN_ENABLED` (defaults to ADMIN_EMAIL being set): backup admin login.
    pub password_login_enabled: bool,
    /// Designated backup admin; optional password bootstraps it when missing.
    pub admin_email: Option<String>,
    pub admin_password: Option<Secret>,
    /// Resolved API key: `AI_API_KEY`, else (Anthropic providers only) `ANTHROPIC_API_KEY`.
    /// AI is enabled when this is set.
    pub ai_api_key: Option<Secret>,
    /// Resolved API origin: `AI_BASE_URL`, else (Anthropic providers only) `ANTHROPIC_BASE_URL`.
    /// Required for `openai-compatible`; optional (mock servers, gateways) for Anthropic.
    pub ai_base_url: Option<String>,
    /// `AI_EXTRA_HEADERS` (`name=value,name2=value2`): sent with every model request.
    /// Values are secrets: never shown in `Debug`.
    pub ai_extra_headers: Vec<(String, Secret)>,
    /// `AI_SESSION_HEADER`: name of a header that carries the import run's id (one id per run).
    pub ai_session_header: Option<String>,
    pub ai_model: String,
    pub ai_daily_quota_per_user: u32,
    pub ai_max_turns: u32,
    /// Output-token budget per model call (`AI_MAX_TOKENS`).
    pub ai_max_tokens: u64,
    /// `AI_PROVIDER` (`anthropic` | `anthropic-compatible` | `openai-compatible` | `openai-responses`).
    pub ai_provider: AiProvider,
    /// `AI_FALLBACKS` (`default` | `off`): server-side model fallback on refusals.
    /// Anthropic only; defaults to on there and off for other providers.
    pub ai_fallbacks: bool,
    /// `AI_STRICT_TOOLS`: strict tool schemas (default: on for `anthropic`, off otherwise).
    pub ai_strict_tools: bool,
    /// `AI_REASONING` (`low` | `off`; default: `low` for `anthropic`, `off` otherwise).
    pub ai_reasoning: AiReasoning,
    /// `AI_REQUEST_TIMEOUT_SECS`: one model call (HTTP request). Default 60 (`anthropic`) / 180 (`anthropic-compatible`).
    pub ai_request_timeout_secs: u64,
    /// `AI_RUN_TIMEOUT_SECS`: a whole import run incl. the retry. Default 90 / 300.
    pub ai_run_timeout_secs: u64,
    /// `AI_DEMO`: scripted demo agent when no API key is set (never in production).
    pub ai_demo: bool,
    pub session_ttl_days: u32,
    pub trusted_proxies: Vec<String>,
    pub client_info_retention_days: u32,
    /// `MAINTENANCE_INTERVAL_HOURS`: period of the background housekeeping job (default 24).
    pub maintenance_interval_hours: u32,
    /// Public endpoints, per client IP: bucket size (burst) ...
    pub public_rate_limit_burst: u32,
    /// ... and refill rate in requests per second.
    pub public_rate_limit_per_sec: f64,
    /// Timezone for deadlines shown in link previews (`DISPLAY_UTC_OFFSET`, default `+08:00`).
    pub display_utc_offset: time::UtcOffset,
    /// Extra origins allowed on state-changing requests (besides `APP_BASE_URL`).
    pub allowed_origins: Vec<String>,
}

impl Config {
    /// Load `.env` (if present) and then read the process environment.
    pub fn from_env() -> Result<Self, ConfigError> {
        let _ = dotenvy::dotenv();
        Self::from_map(&std::env::vars().collect())
    }

    pub fn from_map(env: &HashMap<String, String>) -> Result<Self, ConfigError> {
        let get = |k: &str| env.get(k).map(|v| v.trim()).filter(|v| !v.is_empty());
        let opt = |k: &str| get(k).map(str::to_owned);
        let string = |k: &str, d: &str| get(k).unwrap_or(d).to_owned();
        fn num<T: std::str::FromStr>(
            v: Option<&str>,
            key: &'static str,
            default: T,
        ) -> Result<T, ConfigError> {
            match v {
                None => Ok(default),
                Some(s) => s.parse().map_err(|_| ConfigError::Invalid {
                    key,
                    value: s.to_owned(),
                    reason: "expected a number",
                }),
            }
        }

        let app_env = match get("APP_ENV").map(str::to_ascii_lowercase).as_deref() {
            None | Some("development") => AppEnv::Development,
            Some("production") => AppEnv::Production,
            Some(other) => {
                return Err(ConfigError::Invalid {
                    key: "APP_ENV",
                    value: other.to_owned(),
                    reason: "expected development or production",
                });
            }
        };
        // `None` when the variable is unset or empty.
        let tri_state = |key: &'static str| -> Result<Option<bool>, ConfigError> {
            match get(key).map(str::to_ascii_lowercase).as_deref() {
                None => Ok(None),
                Some("false") | Some("0") | Some("no") | Some("off") => Ok(Some(false)),
                Some("true") | Some("1") | Some("yes") | Some("on") => Ok(Some(true)),
                Some(other) => Err(ConfigError::Invalid {
                    key,
                    value: other.to_owned(),
                    reason: "expected a boolean",
                }),
            }
        };
        let boolean = |key: &'static str| -> Result<bool, ConfigError> {
            Ok(tri_state(key)?.unwrap_or(false))
        };
        let dev_login = boolean("DEV_LOGIN")?;
        let oidc_issuer_url = opt("OIDC_ISSUER_URL");
        let oidc_client_id = opt("OIDC_CLIENT_ID");
        let oidc_client_secret = opt("OIDC_CLIENT_SECRET").map(Secret);
        let oidc_additional_audiences: Vec<String> = opt("OIDC_ADDITIONAL_AUDIENCES")
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect();
        let oidc_enabled = oidc_issuer_url.is_some() && oidc_client_id.is_some();
        if !oidc_enabled
            && (oidc_issuer_url.is_some()
                || oidc_client_id.is_some()
                || oidc_client_secret.is_some()
                || !oidc_additional_audiences.is_empty())
        {
            return Err(ConfigError::Invalid {
                key: "OIDC_ISSUER_URL",
                value: String::new(),
                reason: "OIDC requires both OIDC_ISSUER_URL and OIDC_CLIENT_ID; secret is optional",
            });
        }
        if let Some(issuer) = &oidc_issuer_url {
            let valid = url::Url::parse(issuer).is_ok_and(|u| {
                u.scheme() == "https"
                    && u.host_str().is_some()
                    && u.username().is_empty()
                    && u.password().is_none()
                    && u.query().is_none()
                    && u.fragment().is_none()
            });
            if !valid {
                return Err(ConfigError::Invalid {
                    key: "OIDC_ISSUER_URL",
                    value: issuer.clone(),
                    reason: "expected an HTTPS issuer URL without credentials, query or fragment",
                });
            }
        }
        let (admin_email, admin_password) = match (opt("ADMIN_EMAIL"), opt("ADMIN_PASSWORD")) {
            (None, None) => (None, None),
            (Some(email), Some(password)) => {
                let email = domain::identity::parse_local_email(&email).map_err(|_| {
                    ConfigError::Invalid {
                        key: "ADMIN_EMAIL",
                        value: email.clone(),
                        reason: "expected an email address",
                    }
                })?;
                domain::identity::validate_password(&password).map_err(|_| {
                    ConfigError::Invalid {
                        key: "ADMIN_PASSWORD",
                        value: "<redacted>".into(),
                        reason: "must be 10 to 256 characters",
                    }
                })?;
                (Some(email), Some(Secret(password)))
            }
            (Some(email), None) => (
                Some(domain::identity::parse_local_email(&email).map_err(|_| {
                    ConfigError::Invalid {
                        key: "ADMIN_EMAIL",
                        value: email,
                        reason: "expected an email address",
                    }
                })?),
                None,
            ),
            (None, Some(_)) => {
                return Err(ConfigError::Invalid {
                    key: "ADMIN_EMAIL",
                    value: String::new(),
                    reason: "required when ADMIN_PASSWORD is set",
                });
            }
        };
        let password_login_enabled =
            tri_state("PASSWORD_LOGIN_ENABLED")?.unwrap_or(admin_email.is_some());
        if password_login_enabled && admin_email.is_none() {
            return Err(ConfigError::Invalid {
                key: "ADMIN_EMAIL",
                value: String::new(),
                reason: "required when PASSWORD_LOGIN_ENABLED=true",
            });
        }
        let ai_demo = boolean("AI_DEMO")?;
        let ai_provider = match get("AI_PROVIDER").map(str::to_ascii_lowercase).as_deref() {
            None | Some("anthropic") => AiProvider::Anthropic,
            Some("anthropic-compatible") => AiProvider::AnthropicCompatible,
            Some("openai-compatible") => AiProvider::OpenAiCompatible,
            Some("openai-responses") => AiProvider::OpenAiResponses,
            Some(other) => {
                return Err(ConfigError::Invalid {
                    key: "AI_PROVIDER",
                    value: other.to_owned(),
                    reason: "expected anthropic, anthropic-compatible, openai-compatible or openai-responses",
                });
            }
        };
        // Connection: AI_* applies to every provider and wins; ANTHROPIC_* is the
        // older spelling, still read for the two Anthropic providers only.
        let anthropic_family = !matches!(
            ai_provider,
            AiProvider::OpenAiCompatible | AiProvider::OpenAiResponses
        );
        let responses = ai_provider == AiProvider::OpenAiResponses;
        let from_anthropic = |k: &str| if anthropic_family { opt(k) } else { None };
        let ai_api_key = opt("AI_API_KEY")
            .or_else(|| from_anthropic("ANTHROPIC_API_KEY"))
            .map(Secret);
        let ai_base_url = opt("AI_BASE_URL").or_else(|| from_anthropic("ANTHROPIC_BASE_URL"));
        if !anthropic_family {
            if ai_api_key.is_none() && opt("ANTHROPIC_API_KEY").is_some() {
                return Err(ConfigError::Invalid {
                    key: "AI_API_KEY",
                    value: String::new(),
                    reason: if responses {
                        "required with AI_PROVIDER=openai-responses (ANTHROPIC_API_KEY is only read for the Anthropic providers)"
                    } else {
                        "required with AI_PROVIDER=openai-compatible (ANTHROPIC_API_KEY is only read for the Anthropic providers)"
                    },
                });
            }
            if ai_api_key.is_some() && get("AI_MODEL").is_none() {
                return Err(ConfigError::Invalid {
                    key: "AI_MODEL",
                    value: String::new(),
                    reason: if responses {
                        "required with AI_PROVIDER=openai-responses (the default is a Claude model)"
                    } else {
                        "required with AI_PROVIDER=openai-compatible (the default is a Claude model)"
                    },
                });
            }
            if ai_api_key.is_some() && ai_base_url.is_none() {
                return Err(ConfigError::Invalid {
                    key: "AI_BASE_URL",
                    value: String::new(),
                    reason: if responses {
                        "required with AI_PROVIDER=openai-responses"
                    } else {
                        "required with AI_PROVIDER=openai-compatible"
                    },
                });
            }
        }
        let header_name = |key: &'static str, name: &str| -> Result<(), ConfigError> {
            let ok = !name.is_empty()
                && name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
            ok.then_some(()).ok_or_else(|| ConfigError::Invalid {
                key,
                value: name.to_owned(),
                reason: "expected a header name (letters, digits, - and _)",
            })
        };
        let ai_session_header = opt("AI_SESSION_HEADER");
        if let Some(name) = &ai_session_header {
            header_name("AI_SESSION_HEADER", name)?;
        }
        // Values are never echoed in errors: they may be credentials.
        let mut ai_extra_headers = Vec::new();
        for entry in get("AI_EXTRA_HEADERS").unwrap_or("").split(',') {
            let entry = entry.trim();
            if entry.is_empty() {
                continue;
            }
            let bad = |reason: &'static str| ConfigError::Invalid {
                key: "AI_EXTRA_HEADERS",
                value: "<redacted>".into(),
                reason,
            };
            let (name, value) = entry
                .split_once('=')
                .ok_or_else(|| bad("expected name=value,name2=value2"))?;
            let (name, value) = (name.trim(), value.trim());
            header_name("AI_EXTRA_HEADERS", name).map_err(|_| bad("invalid header name"))?;
            if value.is_empty() || !value.bytes().all(|b| (0x20..0x7f).contains(&b)) {
                return Err(bad("header values must be non-empty printable ASCII"));
            }
            ai_extra_headers.push((name.to_owned(), Secret(value.to_owned())));
        }
        let claude = ai_provider == AiProvider::Anthropic;
        let ai_fallbacks = match get("AI_FALLBACKS").map(str::to_ascii_lowercase).as_deref() {
            None => claude,
            Some("off") => false,
            Some("default") if claude => true,
            Some("default") => {
                return Err(ConfigError::Invalid {
                    key: "AI_FALLBACKS",
                    value: "default".to_owned(),
                    reason: "server-side fallbacks exist only with AI_PROVIDER=anthropic",
                });
            }
            Some(other) => {
                return Err(ConfigError::Invalid {
                    key: "AI_FALLBACKS",
                    value: other.to_owned(),
                    reason: "expected default or off",
                });
            }
        };
        let ai_strict_tools = match get("AI_STRICT_TOOLS") {
            None => claude,
            Some(_) => boolean("AI_STRICT_TOOLS")?,
        };
        let ai_reasoning = match get("AI_REASONING").map(str::to_ascii_lowercase).as_deref() {
            None if claude => AiReasoning::Low,
            None | Some("off") => AiReasoning::Off,
            Some("low") => AiReasoning::Low,
            Some(other) => {
                return Err(ConfigError::Invalid {
                    key: "AI_REASONING",
                    value: other.to_owned(),
                    reason: "expected low or off",
                });
            }
        };
        // Slow gateways (quantised models behind a proxy) need far more than Claude does.
        let (default_request, default_run) = if claude { (60, 90) } else { (180, 300) };
        let ai_request_timeout_secs: u64 = num(
            get("AI_REQUEST_TIMEOUT_SECS"),
            "AI_REQUEST_TIMEOUT_SECS",
            default_request,
        )?;
        let ai_run_timeout_secs: u64 = num(
            get("AI_RUN_TIMEOUT_SECS"),
            "AI_RUN_TIMEOUT_SECS",
            default_run,
        )?;
        if ai_request_timeout_secs == 0 {
            return Err(ConfigError::Invalid {
                key: "AI_REQUEST_TIMEOUT_SECS",
                value: "0".into(),
                reason: "must be greater than 0",
            });
        }
        if ai_run_timeout_secs < ai_request_timeout_secs {
            return Err(ConfigError::Invalid {
                key: "AI_RUN_TIMEOUT_SECS",
                value: ai_run_timeout_secs.to_string(),
                reason: "must be at least AI_REQUEST_TIMEOUT_SECS",
            });
        }

        let list = |k: &str| -> Option<Vec<String>> {
            get(k).map(|v| {
                v.split(',')
                    .map(|s| s.trim().trim_end_matches('/').to_owned())
                    .filter(|s| !s.is_empty())
                    .collect()
            })
        };
        // The Vite dev server (another origin) must be able to call the API in development.
        let allowed_origins = list("ALLOWED_ORIGINS").unwrap_or_else(|| {
            if app_env == AppEnv::Development {
                vec!["http://localhost:5173".to_owned()]
            } else {
                Vec::new()
            }
        });

        if app_env == AppEnv::Production && dev_login {
            return Err(ConfigError::Invalid {
                key: "DEV_LOGIN",
                value: "true".into(),
                reason: "must not be enabled when APP_ENV=production",
            });
        }

        if app_env == AppEnv::Production && ai_demo {
            return Err(ConfigError::Invalid {
                key: "AI_DEMO",
                value: "true".into(),
                reason: "must not be enabled when APP_ENV=production",
            });
        }

        Ok(Self {
            port: num(get("PORT"), "PORT", 3000)?,
            app_env,
            dev_login,
            app_base_url: string("APP_BASE_URL", "http://localhost:3000"),
            database_url: string("DATABASE_URL", "sqlite://data/bento.db"),
            oidc_issuer_url,
            oidc_client_id,
            oidc_client_secret,
            oidc_additional_audiences,
            oidc_enabled,
            password_login_enabled,
            admin_email,
            admin_password,
            ai_api_key,
            ai_base_url,
            ai_extra_headers,
            ai_session_header,
            ai_model: string("AI_MODEL", "claude-opus-5-5"),
            ai_daily_quota_per_user: num(
                get("AI_DAILY_QUOTA_PER_USER"),
                "AI_DAILY_QUOTA_PER_USER",
                30,
            )?,
            ai_max_turns: num(get("AI_MAX_TURNS"), "AI_MAX_TURNS", 8)?,
            ai_max_tokens: {
                let tokens = num(get("AI_MAX_TOKENS"), "AI_MAX_TOKENS", DEFAULT_MAX_TOKENS)?;
                if tokens == 0 {
                    return Err(ConfigError::Invalid {
                        key: "AI_MAX_TOKENS",
                        value: "0".into(),
                        reason: "must be greater than 0",
                    });
                }
                tokens
            },
            ai_provider,
            ai_fallbacks,
            ai_strict_tools,
            ai_reasoning,
            ai_request_timeout_secs,
            ai_run_timeout_secs,
            ai_demo,
            session_ttl_days: num(get("SESSION_TTL_DAYS"), "SESSION_TTL_DAYS", 14)?,
            trusted_proxies: get("TRUSTED_PROXIES")
                .map(|v| {
                    v.split(',')
                        .map(|s| s.trim().to_owned())
                        .filter(|s| !s.is_empty())
                        .collect()
                })
                .unwrap_or_default(),
            client_info_retention_days: num(
                get("CLIENT_INFO_RETENTION_DAYS"),
                "CLIENT_INFO_RETENTION_DAYS",
                90,
            )?,
            maintenance_interval_hours: {
                let h = num(
                    get("MAINTENANCE_INTERVAL_HOURS"),
                    "MAINTENANCE_INTERVAL_HOURS",
                    24u32,
                )?;
                if h == 0 {
                    return Err(ConfigError::Invalid {
                        key: "MAINTENANCE_INTERVAL_HOURS",
                        value: "0".into(),
                        reason: "must be at least 1",
                    });
                }
                h
            },
            public_rate_limit_burst: num(
                get("PUBLIC_RATE_LIMIT_BURST"),
                "PUBLIC_RATE_LIMIT_BURST",
                30,
            )?,
            public_rate_limit_per_sec: num(
                get("PUBLIC_RATE_LIMIT_PER_SEC"),
                "PUBLIC_RATE_LIMIT_PER_SEC",
                2.0,
            )?,
            allowed_origins,
            display_utc_offset: parse_offset(get("DISPLAY_UTC_OFFSET").unwrap_or("+08:00"))?,
        })
    }

    pub fn is_production(&self) -> bool {
        self.app_env == AppEnv::Production
    }

    /// Which AI agent to run: the real one with an API key, else the scripted
    /// demo agent when `AI_DEMO=true` (never in production), else none.
    pub fn ai_mode(&self) -> AiMode {
        if self.ai_api_key.is_some() {
            AiMode::Live
        } else if self.ai_demo && !self.is_production() {
            AiMode::Demo
        } else {
            AiMode::Disabled
        }
    }
}

fn parse_offset(v: &str) -> Result<time::UtcOffset, ConfigError> {
    let parse = || -> Option<time::UtcOffset> {
        let v = v.trim();
        let sign: i8 = match v.as_bytes().first()? {
            b'+' => 1,
            b'-' => -1,
            _ => return None,
        };
        let (h, m) = v[1..].split_once(':')?;
        if h.len() != 2 || m.len() != 2 {
            return None;
        }
        let (h, m): (i8, i8) = (h.parse().ok()?, m.parse().ok()?);
        time::UtcOffset::from_hms(sign * h, sign * m, 0).ok()
    };
    parse().ok_or_else(|| ConfigError::Invalid {
        key: "DISPLAY_UTC_OFFSET",
        value: v.into(),
        reason: "expected a UTC offset like +08:00",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults() {
        let c = Config::from_map(&HashMap::new()).unwrap();
        assert_eq!(c.port, 3000);
        assert_eq!(c.app_env, AppEnv::Development);
        assert!(!c.dev_login);
        assert!(c.oidc_client_id.is_none() && c.ai_api_key.is_none());
        assert!(c.trusted_proxies.is_empty());
        assert_eq!(c.public_rate_limit_burst, 30);
        assert_eq!(
            (c.client_info_retention_days, c.maintenance_interval_hours),
            (90, 24)
        );
        assert_eq!(c.public_rate_limit_per_sec, 2.0);
        assert_eq!(c.display_utc_offset.whole_hours(), 8);
        let m: HashMap<String, String> =
            [("DISPLAY_UTC_OFFSET".to_string(), "-05:30".to_string())].into();
        assert_eq!(
            Config::from_map(&m)
                .unwrap()
                .display_utc_offset
                .whole_minutes(),
            -330
        );
        let bad: HashMap<String, String> =
            [("DISPLAY_UTC_OFFSET".to_string(), "Taipei".to_string())].into();
        assert!(Config::from_map(&bad).is_err());
    }

    #[test]
    fn overrides_and_errors() {
        let m = |kv: &[(&str, &str)]| -> HashMap<String, String> {
            kv.iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect()
        };
        let c = Config::from_map(&m(&[
            ("PORT", "8080"),
            ("APP_ENV", "production"),
            ("TRUSTED_PROXIES", "10.0.0.0/8, 127.0.0.1"),
        ]))
        .unwrap();
        assert_eq!(c.port, 8080);
        assert!(c.is_production() && !c.dev_login);
        assert_eq!(c.trusted_proxies.len(), 2);
        assert!(Config::from_map(&m(&[("PORT", "x")])).is_err());
        assert!(Config::from_map(&m(&[("APP_ENV", "staging")])).is_err());
    }

    #[test]
    fn production_refuses_dev_login() {
        let m: HashMap<String, String> = [("APP_ENV", "production"), ("DEV_LOGIN", "true")]
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let err = Config::from_map(&m).unwrap_err();
        assert!(matches!(
            err,
            ConfigError::Invalid {
                key: "DEV_LOGIN",
                ..
            }
        ));
    }

    #[test]
    fn ai_settings_and_agent_selection() {
        let m = |kv: &[(&str, &str)]| -> HashMap<String, String> {
            kv.iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect()
        };
        let c = Config::from_map(&HashMap::new()).unwrap();
        assert_eq!(c.ai_model, "claude-opus-5-5");
        assert_eq!((c.ai_max_turns, c.ai_daily_quota_per_user), (8, 30));
        assert!(c.ai_fallbacks && !c.ai_demo);
        assert!(c.ai_base_url.is_none());
        assert_eq!(c.ai_mode(), AiMode::Disabled);

        let c = Config::from_map(&m(&[("AI_DEMO", "true")])).unwrap();
        assert_eq!(c.ai_mode(), AiMode::Demo);
        // a key wins over demo mode
        let c = Config::from_map(&m(&[("AI_DEMO", "true"), ("ANTHROPIC_API_KEY", "k")])).unwrap();
        assert_eq!(c.ai_mode(), AiMode::Live);
        let c = Config::from_map(&m(&[
            ("APP_ENV", "production"),
            ("ANTHROPIC_API_KEY", "k"),
            ("AI_FALLBACKS", "off"),
            ("ANTHROPIC_BASE_URL", "http://127.0.0.1:9"),
        ]))
        .unwrap();
        assert_eq!(c.ai_mode(), AiMode::Live);
        assert!(!c.ai_fallbacks);
        assert_eq!(c.ai_base_url.as_deref(), Some("http://127.0.0.1:9"));
        assert!(Config::from_map(&m(&[("AI_FALLBACKS", "maybe")])).is_err());
        assert!(Config::from_map(&m(&[("AI_DEMO", "perhaps")])).is_err());
    }

    #[test]
    fn ai_timeouts_defaults_and_validation() {
        let c = Config::from_map(&HashMap::new()).unwrap();
        assert_eq!((c.ai_request_timeout_secs, c.ai_run_timeout_secs), (60, 90));
        let c = Config::from_map(&env(&[("AI_PROVIDER", "anthropic-compatible")])).unwrap();
        assert_eq!(
            (c.ai_request_timeout_secs, c.ai_run_timeout_secs),
            (180, 300)
        );
        let c = Config::from_map(&env(&[
            ("AI_REQUEST_TIMEOUT_SECS", "120"),
            ("AI_RUN_TIMEOUT_SECS", "120"),
        ]))
        .unwrap();
        assert_eq!(
            (c.ai_request_timeout_secs, c.ai_run_timeout_secs),
            (120, 120)
        );
        for bad in [
            [
                ("AI_REQUEST_TIMEOUT_SECS", "0"),
                ("AI_RUN_TIMEOUT_SECS", "90"),
            ],
            [
                ("AI_REQUEST_TIMEOUT_SECS", "100"),
                ("AI_RUN_TIMEOUT_SECS", "90"),
            ],
            [
                ("AI_REQUEST_TIMEOUT_SECS", "abc"),
                ("AI_RUN_TIMEOUT_SECS", "90"),
            ],
        ] {
            assert!(Config::from_map(&env(&bad)).is_err(), "{bad:?}");
        }
        // raising only the request timeout above the default run timeout is rejected
        assert!(Config::from_map(&env(&[("AI_REQUEST_TIMEOUT_SECS", "100")])).is_err());
    }

    #[test]
    fn ai_max_tokens_defaults_and_validation() {
        for value in [None, Some(""), Some("  ")] {
            let vars = value.map_or_else(HashMap::new, |v| env(&[("AI_MAX_TOKENS", v)]));
            assert_eq!(Config::from_map(&vars).unwrap().ai_max_tokens, 16_384);
        }
        for (value, expected) in [
            (" 32768 ", 32_768),
            ("1", 1),
            ("18446744073709551615", u64::MAX),
        ] {
            let c = Config::from_map(&env(&[("AI_MAX_TOKENS", value)])).unwrap();
            assert_eq!(c.ai_max_tokens, expected);
        }
        for value in ["0", "-1", "1.5", "abc", "18446744073709551616"] {
            assert!(
                matches!(
                    Config::from_map(&env(&[("AI_MAX_TOKENS", value)])).unwrap_err(),
                    ConfigError::Invalid {
                        key: "AI_MAX_TOKENS",
                        ..
                    }
                ),
                "{value}"
            );
        }
    }

    fn env(kv: &[(&str, &str)]) -> HashMap<String, String> {
        kv.iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn oidc_additional_audiences_are_trimmed_and_default_to_empty() {
        assert!(
            Config::from_map(&env(&[]))
                .unwrap()
                .oidc_additional_audiences
                .is_empty()
        );
        let config = Config::from_map(&env(&[
            ("OIDC_ISSUER_URL", "https://id.example"),
            ("OIDC_CLIENT_ID", "cid"),
            ("OIDC_ADDITIONAL_AUDIENCES", " project-1, project-2 ,, "),
        ]))
        .unwrap();
        assert_eq!(config.oidc_additional_audiences, ["project-1", "project-2"]);
        assert!(Config::from_map(&env(&[("OIDC_ADDITIONAL_AUDIENCES", "project-1")])).is_err());
        let empty = Config::from_map(&env(&[("OIDC_ADDITIONAL_AUDIENCES", " , , ")])).unwrap();
        assert!(!empty.oidc_enabled && empty.oidc_additional_audiences.is_empty());
    }

    #[test]
    fn oidc_configuration_supports_public_and_confidential_clients() {
        const CREDS: [(&str, &str); 2] = [
            ("OIDC_CLIENT_ID", "id"),
            ("OIDC_ISSUER_URL", "https://id.example"),
        ];
        let with = |extra: &[(&str, &str)]| {
            let mut kv = CREDS.to_vec();
            kv.extend_from_slice(extra);
            Config::from_map(&env(&kv))
        };
        assert!(!Config::from_map(&env(&[])).unwrap().oidc_enabled);
        let public = with(&[]).unwrap();
        assert!(public.oidc_enabled && public.oidc_client_secret.is_none());
        let confidential = with(&[("OIDC_CLIENT_SECRET", "secret-value")]).unwrap();
        assert!(confidential.oidc_enabled);
        assert!(!format!("{confidential:?}").contains("secret-value"));
        for kv in [
            vec![("OIDC_CLIENT_SECRET", "secret")],
            vec![("OIDC_CLIENT_ID", "id")],
            vec![("OIDC_ISSUER_URL", "https://id.example")],
        ] {
            let err = Config::from_map(&env(&kv)).unwrap_err();
            assert!(matches!(
                err,
                ConfigError::Invalid {
                    key: "OIDC_ISSUER_URL",
                    ..
                }
            ));
        }
        for issuer in [
            "http://id.example",
            "https://user:pass@id.example",
            "https://id.example?x=1",
            "https://id.example/#x",
            "not-a-url",
        ] {
            assert!(with(&[("OIDC_ISSUER_URL", issuer)]).is_err());
        }
        assert!(with(&[("OIDC_ISSUER_URL", "https://id.example/realm/team")]).is_ok());
        // Removed Google variables do not enable login.
        assert!(
            !Config::from_map(&env(&[
                ("GOOGLE_CLIENT_ID", "old"),
                ("GOOGLE_CLIENT_SECRET", "old")
            ]))
            .unwrap()
            .oidc_enabled
        );
    }

    #[test]
    fn password_login_requires_designated_admin() {
        assert!(!Config::from_map(&env(&[])).unwrap().password_login_enabled);
        assert!(
            Config::from_map(&env(&[("ADMIN_EMAIL", "root@example.com")]))
                .unwrap()
                .password_login_enabled
        );
        assert!(Config::from_map(&env(&[("PASSWORD_LOGIN_ENABLED", "true")])).is_err());
        let c = Config::from_map(&env(&[("PASSWORD_LOGIN_ENABLED", "false")])).unwrap();
        assert!(!c.password_login_enabled);
        assert!(Config::from_map(&env(&[("PASSWORD_LOGIN_ENABLED", "x")])).is_err());
    }

    #[test]
    fn admin_bootstrap_vars_come_in_pairs() {
        let c = Config::from_map(&env(&[])).unwrap();
        assert!(c.admin_email.is_none() && c.admin_password.is_none());
        let c = Config::from_map(&env(&[
            ("ADMIN_EMAIL", " Root@Example.com "),
            ("ADMIN_PASSWORD", "long enough pw"),
        ]))
        .unwrap();
        assert_eq!(c.admin_email.as_deref(), Some("root@example.com"));
        assert_eq!(
            c.admin_password.as_ref().unwrap().expose(),
            "long enough pw"
        );
        // the password never shows in Debug output
        assert!(!format!("{c:?}").contains("long enough pw"));

        for kv in [
            vec![("ADMIN_PASSWORD", "long enough pw")],
            vec![
                ("ADMIN_EMAIL", "not-an-email"),
                ("ADMIN_PASSWORD", "long enough pw"),
            ],
            vec![
                ("ADMIN_EMAIL", "root@example.com"),
                ("ADMIN_PASSWORD", "short"),
            ],
        ] {
            let err = Config::from_map(&env(&kv)).unwrap_err();
            assert!(!err.to_string().contains("long enough pw"));
            assert!(!err.to_string().contains("short\""));
        }
    }

    #[test]
    fn ai_provider_options_and_defaults() {
        let m = |kv: &[(&str, &str)]| -> HashMap<String, String> {
            kv.iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect()
        };
        // Claude defaults are unchanged
        let c = Config::from_map(&HashMap::new()).unwrap();
        assert_eq!(c.ai_provider, AiProvider::Anthropic);
        assert!(c.ai_fallbacks && c.ai_strict_tools);
        assert_eq!(c.ai_reasoning, AiReasoning::Low);

        // a compatible gateway gets the plain subset by default
        let c = Config::from_map(&m(&[("AI_PROVIDER", "Anthropic-Compatible")])).unwrap();
        assert_eq!(c.ai_provider, AiProvider::AnthropicCompatible);
        assert!(!c.ai_fallbacks && !c.ai_strict_tools);
        assert_eq!(c.ai_reasoning, AiReasoning::Off);

        // ... with individual opt-ins
        let c = Config::from_map(&m(&[
            ("AI_PROVIDER", "anthropic-compatible"),
            ("AI_STRICT_TOOLS", "true"),
            ("AI_REASONING", "low"),
            ("AI_FALLBACKS", "off"),
        ]))
        .unwrap();
        assert!(c.ai_strict_tools && !c.ai_fallbacks);
        assert_eq!(c.ai_reasoning, AiReasoning::Low);

        // ... and each can be switched off on Anthropic
        let c =
            Config::from_map(&m(&[("AI_STRICT_TOOLS", "false"), ("AI_REASONING", "off")])).unwrap();
        assert!(!c.ai_strict_tools);
        assert_eq!(c.ai_reasoning, AiReasoning::Off);

        // invalid combinations and values
        let err = Config::from_map(&m(&[
            ("AI_PROVIDER", "anthropic-compatible"),
            ("AI_FALLBACKS", "default"),
        ]))
        .unwrap_err();
        assert!(matches!(
            err,
            ConfigError::Invalid {
                key: "AI_FALLBACKS",
                ..
            }
        ));
        for (k, v) in [
            ("AI_PROVIDER", "openai"),
            ("AI_REASONING", "high"),
            ("AI_STRICT_TOOLS", "maybe"),
        ] {
            assert!(Config::from_map(&m(&[(k, v)])).is_err(), "{k}={v}");
        }
    }

    #[test]
    fn ai_connection_precedence_and_openai_compatible() {
        let m = |kv: &[(&str, &str)]| -> HashMap<String, String> {
            kv.iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect()
        };
        // AI_* wins over the legacy ANTHROPIC_* spelling on the Anthropic providers
        let c = Config::from_map(&m(&[
            ("AI_API_KEY", "new"),
            ("ANTHROPIC_API_KEY", "old"),
            ("AI_BASE_URL", "http://new"),
            ("ANTHROPIC_BASE_URL", "http://old"),
        ]))
        .unwrap();
        assert_eq!(c.ai_api_key.as_ref().unwrap().expose(), "new");
        assert_eq!(c.ai_base_url.as_deref(), Some("http://new"));
        // ... and AI_* alone enables AI
        let c = Config::from_map(&m(&[("AI_API_KEY", "k")])).unwrap();
        assert_eq!(c.ai_mode(), AiMode::Live);

        // openai-compatible: key, base URL and model are all required
        let ok = [
            ("AI_PROVIDER", "openai-compatible"),
            ("AI_API_KEY", "k"),
            ("AI_BASE_URL", "https://gw.example/v1"),
            ("AI_MODEL", "deepseek-v4.1-flash"),
        ];
        let c = Config::from_map(&m(&ok)).unwrap();
        assert_eq!(c.ai_provider, AiProvider::OpenAiCompatible);
        assert_eq!(c.ai_mode(), AiMode::Live);
        assert!(!c.ai_fallbacks && !c.ai_strict_tools);
        assert_eq!(c.ai_reasoning, AiReasoning::Off);
        assert_eq!(
            (c.ai_request_timeout_secs, c.ai_run_timeout_secs),
            (180, 300)
        );
        assert!(Config::from_map(&m(&ok[..2])).is_err()); // no base URL / model
        let no_url = [ok[0], ok[1], ok[3]];
        assert!(matches!(
            Config::from_map(&m(&no_url)).unwrap_err(),
            ConfigError::Invalid {
                key: "AI_BASE_URL",
                ..
            }
        ));
        let no_model = [ok[0], ok[1], ok[2]];
        assert!(matches!(
            Config::from_map(&m(&no_model)).unwrap_err(),
            ConfigError::Invalid {
                key: "AI_MODEL",
                ..
            }
        ));
        // ANTHROPIC_* is never a fallback here
        let c = Config::from_map(&m(&[
            ("AI_PROVIDER", "openai-compatible"),
            ("AI_BASE_URL", "http://x"),
        ]))
        .unwrap();
        assert_eq!(c.ai_mode(), AiMode::Disabled);
        let err = Config::from_map(&m(&[
            ("AI_PROVIDER", "openai-compatible"),
            ("ANTHROPIC_API_KEY", "old"),
            ("AI_BASE_URL", "http://x"),
            ("AI_MODEL", "m"),
        ]))
        .unwrap_err();
        assert!(matches!(
            err,
            ConfigError::Invalid {
                key: "AI_API_KEY",
                ..
            }
        ));
        // fallbacks are Anthropic-only
        let mut bad = ok.to_vec();
        bad.push(("AI_FALLBACKS", "default"));
        assert!(Config::from_map(&m(&bad)).is_err());
        // opt-ins
        let mut on = ok.to_vec();
        on.extend([("AI_STRICT_TOOLS", "true"), ("AI_REASONING", "low")]);
        let c = Config::from_map(&m(&on)).unwrap();
        assert!(c.ai_strict_tools);
        assert_eq!(c.ai_reasoning, AiReasoning::Low);
    }

    #[test]
    fn openai_responses_shares_the_openai_compatible_rules() {
        let m = |kv: &[(&str, &str)]| -> HashMap<String, String> {
            kv.iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect()
        };
        let ok = [
            ("AI_PROVIDER", "OpenAI-Responses"),
            ("AI_API_KEY", "k"),
            ("AI_BASE_URL", "https://gw.example"),
            ("AI_MODEL", "gpt-6-luna"),
        ];
        let c = Config::from_map(&m(&ok)).unwrap();
        assert_eq!(c.ai_provider, AiProvider::OpenAiResponses);
        assert_eq!(c.ai_mode(), AiMode::Live);
        assert!(!c.ai_fallbacks && !c.ai_strict_tools);
        assert_eq!(c.ai_reasoning, AiReasoning::Off);
        assert_eq!(
            (c.ai_request_timeout_secs, c.ai_run_timeout_secs),
            (180, 300)
        );
        for missing in ["AI_BASE_URL", "AI_MODEL"] {
            let kv: Vec<_> = ok.iter().copied().filter(|(k, _)| *k != missing).collect();
            assert!(
                matches!(
                    Config::from_map(&m(&kv)).unwrap_err(),
                    ConfigError::Invalid { key, .. } if key == missing
                ),
                "{missing}"
            );
        }
        // ANTHROPIC_* is never a fallback; fallbacks are Anthropic-only
        let err = Config::from_map(&m(&[
            ("AI_PROVIDER", "openai-responses"),
            ("ANTHROPIC_API_KEY", "old"),
            ("AI_BASE_URL", "http://x"),
            ("AI_MODEL", "m"),
        ]))
        .unwrap_err();
        assert!(matches!(
            err,
            ConfigError::Invalid {
                key: "AI_API_KEY",
                ..
            }
        ));
        let mut bad = ok.to_vec();
        bad.push(("AI_FALLBACKS", "default"));
        assert!(Config::from_map(&m(&bad)).is_err());
        let mut on = ok.to_vec();
        on.extend([("AI_STRICT_TOOLS", "true"), ("AI_REASONING", "low")]);
        let c = Config::from_map(&m(&on)).unwrap();
        assert!(c.ai_strict_tools);
        assert_eq!(c.ai_reasoning, AiReasoning::Low);
    }

    #[test]
    fn ai_extra_and_session_headers() {
        let m = |kv: &[(&str, &str)]| -> HashMap<String, String> {
            kv.iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect()
        };
        let c = Config::from_map(&HashMap::new()).unwrap();
        assert!(c.ai_extra_headers.is_empty() && c.ai_session_header.is_none());
        let c = Config::from_map(&m(&[
            ("AI_EXTRA_HEADERS", "x-a=1, X-Token = s3cret=,"),
            ("AI_SESSION_HEADER", "x-opencode-session"),
        ]))
        .unwrap();
        let pairs: Vec<_> = c
            .ai_extra_headers
            .iter()
            .map(|(n, v)| (n.as_str(), v.expose()))
            .collect();
        assert_eq!(pairs, [("x-a", "1"), ("X-Token", "s3cret=")]);
        assert_eq!(c.ai_session_header.as_deref(), Some("x-opencode-session"));
        // values never reach Debug output
        assert!(!format!("{c:?}").contains("s3cret"));
        for bad in ["novalue", "=v", "bad name=v", "a=", "a=é"] {
            let err = Config::from_map(&m(&[("AI_EXTRA_HEADERS", bad)])).unwrap_err();
            assert!(!err.to_string().contains(bad), "echoed {bad}");
        }
        assert!(Config::from_map(&m(&[("AI_SESSION_HEADER", "bad name")])).is_err());
    }

    #[test]
    fn production_refuses_ai_demo() {
        let m: HashMap<String, String> = [("APP_ENV", "production"), ("AI_DEMO", "true")]
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let err = Config::from_map(&m).unwrap_err();
        assert!(matches!(err, ConfigError::Invalid { key: "AI_DEMO", .. }));
    }

    #[test]
    fn allowed_origins_defaults_and_parsing() {
        let c = Config::from_map(&HashMap::new()).unwrap();
        assert_eq!(c.allowed_origins, ["http://localhost:5173"]);
        let m = |env: &str, ao: Option<&str>| -> HashMap<String, String> {
            let mut h = HashMap::from([("APP_ENV".to_string(), env.to_string())]);
            if let Some(v) = ao {
                h.insert("ALLOWED_ORIGINS".into(), v.into());
            }
            h
        };
        assert!(
            Config::from_map(&m("production", None))
                .unwrap()
                .allowed_origins
                .is_empty()
        );
        let c =
            Config::from_map(&m("development", Some("https://a.test/, http://b.test"))).unwrap();
        assert_eq!(c.allowed_origins, ["https://a.test", "http://b.test"]);
    }
}
