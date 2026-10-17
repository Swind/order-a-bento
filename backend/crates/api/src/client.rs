//! Client identification for the public endpoints: the real client IP
//! (honouring `X-Forwarded-For` only behind trusted proxies), `ClientInfo`
//! assembly and per-IP rate limiting.

use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, Instant};

use axum::extract::{ConnectInfo, FromRequestParts};
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use domain::ordering::ClientInfo;
use ipnet::IpNet;

use crate::error::AppError;
use crate::state::AppState;

/// Parse `TRUSTED_PROXIES` entries: CIDRs (`10.0.0.0/8`) or bare addresses.
pub fn parse_trusted_proxies(entries: &[String]) -> Result<Vec<IpNet>, String> {
    entries
        .iter()
        .map(|e| {
            e.parse::<IpNet>()
                .or_else(|_| e.parse::<IpAddr>().map(IpNet::from))
                .map_err(|_| format!("invalid TRUSTED_PROXIES entry: {e:?}"))
        })
        .collect()
}

fn is_trusted(ip: IpAddr, trusted: &[IpNet]) -> bool {
    trusted.iter().any(|n| n.contains(&ip))
}

/// The client address.
///
/// `X-Forwarded-For` is consulted only when the TCP peer itself is a trusted
/// proxy; then the right-most entry that is not a trusted proxy wins (entries
/// further left are client-controlled). An unparsable entry stops the walk and
/// falls back to the peer, as does a list made only of trusted proxies.
pub fn client_ip(peer: Option<IpAddr>, xff: &[&str], trusted: &[IpNet]) -> Option<IpAddr> {
    let peer = peer?.to_canonical();
    if !is_trusted(peer, trusted) {
        return Some(peer);
    }
    for entry in xff
        .iter()
        .flat_map(|v| v.split(','))
        .rev()
        .map(str::trim)
        .filter(|e| !e.is_empty())
    {
        match entry.parse::<IpAddr>() {
            Ok(ip) => {
                let ip = ip.to_canonical();
                if !is_trusted(ip, trusted) {
                    return Some(ip);
                }
            }
            Err(_) => return Some(peer),
        }
    }
    Some(peer)
}

pub(crate) fn rate_limited(retry_after: Duration) -> Response {
    let mut res = AppError::Coded {
        status: StatusCode::TOO_MANY_REQUESTS,
        code: "rate_limited",
        detail: "Too many requests; slow down and retry shortly.".into(),
    }
    .into_response();
    let secs = retry_after.as_secs_f64().ceil().max(1.0) as u64;
    if let Ok(v) = HeaderValue::from_str(&secs.to_string()) {
        res.headers_mut().insert(header::RETRY_AFTER, v);
    }
    res
}

/// Extractor for public endpoints. Resolves the client IP and `User-Agent`
/// and enforces the per-IP rate limit (429 `rate_limited`) before any other
/// extractor or handler work happens.
#[derive(Debug, Clone)]
pub struct ClientCtx {
    pub ip: Option<IpAddr>,
    pub user_agent: Option<String>,
}

impl ClientCtx {
    /// Server-derived `ClientInfo` fields; the caller adds the client-supplied ones.
    pub fn client_info(&self, state: &AppState) -> ClientInfo {
        ClientInfo {
            ip: self.ip.map(|i| i.to_string()),
            user_agent: self.user_agent.clone(),
            ua_summary: self
                .user_agent
                .as_deref()
                .and_then(|ua| state.ua_parser.summarize(ua)),
            ..Default::default()
        }
    }
}

fn header_values(headers: &HeaderMap, name: header::HeaderName) -> Vec<&str> {
    headers
        .get_all(name)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .collect()
}

impl FromRequestParts<AppState> for ClientCtx {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let peer = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|c| c.0.ip());
        let xff = header_values(
            &parts.headers,
            header::HeaderName::from_static("x-forwarded-for"),
        );
        let ip = client_ip(peer, &xff, &state.web.trusted_proxies);
        if let Err(retry) = state.limiter.check(ip, Instant::now()) {
            return Err(rate_limited(retry));
        }
        let user_agent = parts
            .headers
            .get(header::USER_AGENT)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        Ok(Self { ip, user_agent })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nets(v: &[&str]) -> Vec<IpNet> {
        parse_trusted_proxies(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>()).unwrap()
    }
    fn ip(s: &str) -> Option<IpAddr> {
        Some(s.parse().unwrap())
    }

    #[test]
    fn untrusted_peer_ignores_xff() {
        let t = nets(&["10.0.0.0/8"]);
        assert_eq!(
            client_ip(ip("203.0.113.9"), &["6.6.6.6"], &t),
            ip("203.0.113.9")
        );
        assert_eq!(
            client_ip(ip("203.0.113.9"), &["6.6.6.6"], &[]),
            ip("203.0.113.9")
        );
        assert_eq!(client_ip(None, &["6.6.6.6"], &t), None);
    }

    #[test]
    fn trusted_peer_takes_rightmost_untrusted_entry() {
        let t = nets(&["10.0.0.0/8", "192.168.1.1"]);
        // Client-forged left entry is ignored.
        assert_eq!(
            client_ip(ip("10.0.0.5"), &["9.9.9.9, 198.51.100.7, 10.1.1.1"], &t),
            ip("198.51.100.7")
        );
        // Several header lines are joined in order.
        assert_eq!(
            client_ip(
                ip("10.0.0.5"),
                &["9.9.9.9", "198.51.100.7, 192.168.1.1"],
                &t
            ),
            ip("198.51.100.7")
        );
        // Only trusted entries / none / garbage -> the peer.
        assert_eq!(client_ip(ip("10.0.0.5"), &["10.2.2.2"], &t), ip("10.0.0.5"));
        assert_eq!(client_ip(ip("10.0.0.5"), &[], &t), ip("10.0.0.5"));
        assert_eq!(
            client_ip(ip("10.0.0.5"), &["1.2.3.4, not-an-ip"], &t),
            ip("10.0.0.5")
        );
    }

    #[test]
    fn ipv4_mapped_ipv6_is_canonicalised() {
        let t = nets(&["10.0.0.0/8"]);
        assert_eq!(
            client_ip(ip("::ffff:10.0.0.5"), &["198.51.100.7"], &t),
            ip("198.51.100.7")
        );
    }

    #[test]
    fn trusted_proxy_parsing() {
        assert_eq!(nets(&["127.0.0.1", "fd00::/8"]).len(), 2);
        assert!(parse_trusted_proxies(&["nope".into()]).is_err());
    }
}
