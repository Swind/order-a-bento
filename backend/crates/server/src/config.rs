//! Environment-driven configuration (design §11).

use std::collections::HashMap;

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
    pub anthropic_api_key: Option<String>,
    /// `ANTHROPIC_BASE_URL`: API origin override (mock servers, gateways).
    pub anthropic_base_url: Option<String>,
    pub ai_model: String,
    pub ai_daily_quota_per_user: u32,
    pub ai_max_turns: u32,
    /// `AI_FALLBACKS` (`default` | `off`): server-side model fallback on refusals.
    pub ai_fallbacks: bool,
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
        let boolean = |key: &'static str| -> Result<bool, ConfigError> {
            match get(key).map(str::to_ascii_lowercase).as_deref() {
                None | Some("false") | Some("0") | Some("no") | Some("off") => Ok(false),
                Some("true") | Some("1") | Some("yes") | Some("on") => Ok(true),
                Some(other) => Err(ConfigError::Invalid {
                    key,
                    value: other.to_owned(),
                    reason: "expected a boolean",
                }),
            }
        };
        let dev_login = boolean("DEV_LOGIN")?;
        let ai_demo = boolean("AI_DEMO")?;
        let ai_fallbacks = match get("AI_FALLBACKS").map(str::to_ascii_lowercase).as_deref() {
            None | Some("default") => true,
            Some("off") => false,
            Some(other) => {
                return Err(ConfigError::Invalid {
                    key: "AI_FALLBACKS",
                    value: other.to_owned(),
                    reason: "expected default or off",
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
            google_client_id: opt("GOOGLE_CLIENT_ID"),
            google_client_secret: opt("GOOGLE_CLIENT_SECRET"),
            anthropic_api_key: opt("ANTHROPIC_API_KEY"),
            anthropic_base_url: opt("ANTHROPIC_BASE_URL"),
            ai_model: string("AI_MODEL", "claude-opus-5-5"),
            ai_daily_quota_per_user: num(
                get("AI_DAILY_QUOTA_PER_USER"),
                "AI_DAILY_QUOTA_PER_USER",
                30,
            )?,
            ai_max_turns: num(get("AI_MAX_TURNS"), "AI_MAX_TURNS", 8)?,
            ai_fallbacks,
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
