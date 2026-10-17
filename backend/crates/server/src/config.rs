//! Environment-driven configuration (design §11).

use std::collections::HashMap;

use infrastructure::ai::{AiProvider, AiReasoning};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppEnv {
    Development,
    Production,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiMode {
    Anthropic,
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
    pub google_client_id: Option<String>,
    pub google_client_secret: Option<String>,
    /// Resolved `GOOGLE_OAUTH_ENABLED`: Google login is available. Always implies both credentials.
    pub google_oauth_enabled: bool,
    /// `PASSWORD_LOGIN_ENABLED` (default true): local-account password login endpoint.
    pub password_login_enabled: bool,
    /// `ADMIN_EMAIL` / `ADMIN_PASSWORD` (both or neither): local admin created at startup if missing.
    pub admin_email: Option<String>,
    pub admin_password: Option<Secret>,
    pub anthropic_api_key: Option<String>,
    /// `ANTHROPIC_BASE_URL`: API origin override (mock servers, gateways).
    pub anthropic_base_url: Option<String>,
    pub ai_model: String,
    pub ai_daily_quota_per_user: u32,
    pub ai_max_turns: u32,
    /// `AI_PROVIDER` (`anthropic` | `anthropic-compatible`).
    pub ai_provider: AiProvider,
    /// `AI_FALLBACKS` (`default` | `off`): server-side model fallback on refusals.
    /// Anthropic only; defaults to on there and off for other providers.
    pub ai_fallbacks: bool,
    /// `AI_STRICT_TOOLS`: strict tool schemas (default: on for `anthropic`, off otherwise).
    pub ai_strict_tools: bool,
    /// `AI_REASONING` (`low` | `off`; default: `low` for `anthropic`, `off` otherwise).
    pub ai_reasoning: AiReasoning,
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
        let google_client_id = opt("GOOGLE_CLIENT_ID");
        let google_client_secret = opt("GOOGLE_CLIENT_SECRET");
        let has_google_credentials = google_client_id.is_some() && google_client_secret.is_some();
        // Unset = auto (on iff both credentials are set); false = off even with credentials.
        let google_oauth_enabled = match tri_state("GOOGLE_OAUTH_ENABLED")? {
            None => has_google_credentials,
            Some(false) => false,
            Some(true) if has_google_credentials => true,
            Some(true) => {
                return Err(ConfigError::Invalid {
                    key: "GOOGLE_OAUTH_ENABLED",
                    value: "true".into(),
                    reason: "requires GOOGLE_CLIENT_ID and GOOGLE_CLIENT_SECRET",
                });
            }
        };
        let password_login_enabled = tri_state("PASSWORD_LOGIN_ENABLED")?.unwrap_or(true);
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
            (Some(_), None) => {
                return Err(ConfigError::Invalid {
                    key: "ADMIN_PASSWORD",
                    value: String::new(),
                    reason: "required when ADMIN_EMAIL is set",
                });
            }
            (None, Some(_)) => {
                return Err(ConfigError::Invalid {
                    key: "ADMIN_EMAIL",
                    value: String::new(),
                    reason: "required when ADMIN_PASSWORD is set",
                });
            }
        };
        let ai_demo = boolean("AI_DEMO")?;
        let ai_provider = match get("AI_PROVIDER").map(str::to_ascii_lowercase).as_deref() {
            None | Some("anthropic") => AiProvider::Anthropic,
            Some("anthropic-compatible") => AiProvider::AnthropicCompatible,
            Some(other) => {
                return Err(ConfigError::Invalid {
                    key: "AI_PROVIDER",
                    value: other.to_owned(),
                    reason: "expected anthropic or anthropic-compatible",
                });
            }
        };
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
            google_client_id,
            google_client_secret,
            google_oauth_enabled,
            password_login_enabled,
            admin_email,
            admin_password,
            anthropic_api_key: opt("ANTHROPIC_API_KEY"),
            anthropic_base_url: opt("ANTHROPIC_BASE_URL"),
            ai_model: string("AI_MODEL", "claude-opus-5-5"),
            ai_daily_quota_per_user: num(
                get("AI_DAILY_QUOTA_PER_USER"),
                "AI_DAILY_QUOTA_PER_USER",
                30,
            )?,
            ai_max_turns: num(get("AI_MAX_TURNS"), "AI_MAX_TURNS", 8)?,
            ai_provider,
            ai_fallbacks,
            ai_strict_tools,
            ai_reasoning,
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
        if self.anthropic_api_key.is_some() {
            AiMode::Anthropic
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
        assert!(c.google_client_id.is_none() && c.anthropic_api_key.is_none());
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
        assert!(c.anthropic_base_url.is_none());
        assert_eq!(c.ai_mode(), AiMode::Disabled);

        let c = Config::from_map(&m(&[("AI_DEMO", "true")])).unwrap();
        assert_eq!(c.ai_mode(), AiMode::Demo);
        // a key wins over demo mode
        let c = Config::from_map(&m(&[("AI_DEMO", "true"), ("ANTHROPIC_API_KEY", "k")])).unwrap();
        assert_eq!(c.ai_mode(), AiMode::Anthropic);
        let c = Config::from_map(&m(&[
            ("APP_ENV", "production"),
            ("ANTHROPIC_API_KEY", "k"),
            ("AI_FALLBACKS", "off"),
            ("ANTHROPIC_BASE_URL", "http://127.0.0.1:9"),
        ]))
        .unwrap();
        assert_eq!(c.ai_mode(), AiMode::Anthropic);
        assert!(!c.ai_fallbacks);
        assert_eq!(c.anthropic_base_url.as_deref(), Some("http://127.0.0.1:9"));
        assert!(Config::from_map(&m(&[("AI_FALLBACKS", "maybe")])).is_err());
        assert!(Config::from_map(&m(&[("AI_DEMO", "perhaps")])).is_err());
    }

    fn env(kv: &[(&str, &str)]) -> HashMap<String, String> {
        kv.iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn google_oauth_enabled_auto_true_false() {
        const CREDS: [(&str, &str); 2] =
            [("GOOGLE_CLIENT_ID", "id"), ("GOOGLE_CLIENT_SECRET", "s")];
        let with = |extra: &[(&str, &str)]| {
            let mut kv = CREDS.to_vec();
            kv.extend_from_slice(extra);
            Config::from_map(&env(&kv))
        };
        // auto: on iff both credentials are set
        assert!(!Config::from_map(&env(&[])).unwrap().google_oauth_enabled);
        assert!(with(&[]).unwrap().google_oauth_enabled);
        let only_id = Config::from_map(&env(&[("GOOGLE_CLIENT_ID", "id")])).unwrap();
        assert!(!only_id.google_oauth_enabled);
        // explicit false wins over credentials
        assert!(
            !with(&[("GOOGLE_OAUTH_ENABLED", "false")])
                .unwrap()
                .google_oauth_enabled
        );
        assert!(
            with(&[("GOOGLE_OAUTH_ENABLED", "TRUE")])
                .unwrap()
                .google_oauth_enabled
        );
        // explicit true needs both credentials
        for kv in [
            vec![("GOOGLE_OAUTH_ENABLED", "true")],
            vec![("GOOGLE_OAUTH_ENABLED", "true"), ("GOOGLE_CLIENT_ID", "id")],
        ] {
            let err = Config::from_map(&env(&kv)).unwrap_err();
            assert!(matches!(
                err,
                ConfigError::Invalid {
                    key: "GOOGLE_OAUTH_ENABLED",
                    ..
                }
            ));
        }
        assert!(Config::from_map(&env(&[("GOOGLE_OAUTH_ENABLED", "maybe")])).is_err());
        // empty = unset
        assert!(
            !Config::from_map(&env(&[("GOOGLE_OAUTH_ENABLED", "")]))
                .unwrap()
                .google_oauth_enabled
        );
    }

    #[test]
    fn password_login_defaults_on() {
        assert!(Config::from_map(&env(&[])).unwrap().password_login_enabled);
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
            vec![("ADMIN_EMAIL", "root@example.com")],
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
