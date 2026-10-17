//! API tests for `/api/v1/group-orders` and `/api/v1/public/g/*`.

use std::net::SocketAddr;

use axum::Router;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{HeaderMap, Request, StatusCode, header};
use serde_json::{Value, json};

use super::directory_tests::{Client, login, raw};
use crate::RateLimitConfig;
use crate::router;
use crate::testing::{TestApp, test_app_with};

const UA_IPHONE_LINE: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 18_1 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Mobile/15E148 Safari Line/14.17.0";

pub(super) struct Fixture {
    pub(super) c: Client,
    pub(super) group: Value,
    pub(super) token: String,
}

impl Fixture {
    pub(super) async fn new() -> Self {
        Self::from_client(Client::new().await).await
    }

    async fn from_client(c: Client) -> Self {
        let (s, roster) = c
            .post(
                "/rosters",
                json!({"name": "午餐組", "members": [{"name": "Amy"}, {"name": "Bob"}, {"name": "Cat"}]}),
            )
            .await;
        assert_eq!(s, StatusCode::CREATED, "{roster}");
        let (s, vendor) = c
            .post(
                "/vendors",
                json!({"name": "好吃便當", "phone": "02-1234", "address": "台北",
                       "menuItems": [
                           {"name": "排骨飯", "price": 100, "category": "飯"},
                           {"name": "雞腿飯", "price": 110},
                           {"name": "素食", "price": 90, "description": "全素"}]}),
            )
            .await;
        assert_eq!(s, StatusCode::CREATED, "{vendor}");
        let (s, group) = c
            .post(
                "/group-orders",
                json!({"title": "週五午餐", "rosterId": roster["id"], "vendorId": vendor["id"], "note": "11:30 前"}),
            )
            .await;
        assert_eq!(s, StatusCode::CREATED, "{group}");
        let url = group["shareUrl"].as_str().unwrap();
        let token = url
            .strip_prefix("http://localhost:3000/g/")
            .unwrap()
            .to_owned();
        assert_eq!(token.len(), 43);
        Self { c, group, token }
    }

    pub(super) fn pid(&self, name: &str) -> String {
        self.group["participants"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == name)
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    pub(super) fn item(&self, name: &str) -> String {
        self.group["offeredItems"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == name)
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    pub(super) fn base(&self) -> String {
        format!("/api/v1/public/g/{}", self.token)
    }

    pub(super) async fn public_get(&self, suffix: &str) -> (StatusCode, Value) {
        let (s, v, _) = send(
            &self.c.app,
            "GET",
            &format!("{}{suffix}", self.base()),
            None,
            None,
            &[],
        )
        .await;
        (s, v)
    }

    pub(super) async fn submit(&self, who: &str, body: Value) -> (StatusCode, Value) {
        let path = format!("{}/participants/{}/submissions", self.base(), self.pid(who));
        let (s, v, _) = send(&self.c.app, "POST", &path, Some(body), None, &[]).await;
        (s, v)
    }

    fn state(&self) -> &crate::AppState {
        &self.c._t.as_ref().unwrap().state
    }
}

pub(super) fn key() -> String {
    uuid::Uuid::now_v7().to_string()
}

pub(super) async fn send(
    app: &Router,
    method: &str,
    path: &str,
    body: Option<Value>,
    peer: Option<&str>,
    headers: &[(&str, &str)],
) -> (StatusCode, Value, HeaderMap) {
    let mut b = Request::builder().method(method).uri(path);
    for (k, v) in headers {
        b = b.header(*k, *v);
    }
    let body = match body {
        Some(v) => {
            b = b.header(header::CONTENT_TYPE, "application/json");
            Body::from(v.to_string())
        }
        None => Body::empty(),
    };
    let mut req = b.body(body).unwrap();
    if let Some(p) = peer {
        let addr = SocketAddr::new(p.parse().unwrap(), 40_000);
        req.extensions_mut().insert(ConnectInfo(addr));
    }
    raw(app, req).await
}

async fn last_submission(
    f: &Fixture,
) -> (
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
) {
    sqlx::query_as(
        "SELECT client_ip, ua_summary, device_id, user_agent FROM order_submissions ORDER BY id DESC LIMIT 1",
    )
    .fetch_one(&f.state().db.read)
    .await
    .unwrap()
}

async fn submission_count(f: &Fixture) -> i64 {
    sqlx::query_as::<_, (i64,)>("SELECT COUNT(*) FROM order_submissions")
        .fetch_one(&f.state().db.read)
        .await
        .unwrap()
        .0
}

#[tokio::test]
async fn organizer_group_detail_and_scoping() {
    let f = Fixture::new().await;
    let g = &f.group;
    assert_eq!(g["title"], "週五午餐");
    assert_eq!(g["status"], "open");
    assert_eq!(g["deadlineAt"], Value::Null);
    assert_eq!(g["note"], "11:30 前");
    assert_eq!(g["closedAt"], Value::Null);
    assert_eq!(
        g["vendor"],
        json!({"name": "好吃便當", "phone": "02-1234", "address": "台北"})
    );
    assert_eq!(g["participants"].as_array().unwrap().len(), 3);
    assert_eq!(g["participants"][1]["name"], "Bob");
    assert_eq!(g["participants"][1]["sortOrder"], 1);
    let items = g["offeredItems"].as_array().unwrap();
    assert_eq!(items.len(), 3);
    assert_eq!(items[0]["price"], 100);
    assert_eq!(items[0]["soldOut"], false);
    assert_eq!(items[2]["description"], "全素");

    let id = g["id"].as_str().unwrap();
    let (s, got) = f.c.get(&format!("/group-orders/{id}")).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(&got, g);

    // Another organizer: 404; unknown / malformed ids: 404; no cookie: 401.
    let other = f.c.other().await;
    let (s, v) = other.get(&format!("/group-orders/{id}")).await;
    assert_eq!(
        (s, v["code"].as_str()),
        (StatusCode::NOT_FOUND, Some("not_found"))
    );
    assert_eq!(f.c.get("/group-orders/nope").await.0, StatusCode::NOT_FOUND);
    assert_eq!(
        f.c.get(&format!("/group-orders/{}", uuid::Uuid::now_v7()))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    // The other organizer cannot open a group from foreign master data.
    let (s, _) = other
        .post(
            "/group-orders",
            json!({"title": "x", "rosterId": "00000000-0000-7000-8000-000000000000", "vendorId": "00000000-0000-7000-8000-000000000001"}),
        )
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (_, rosters) = f.c.get("/rosters").await;
    let (_, vendors) = f.c.get("/vendors").await;
    let (s, _) = other
        .post(
            "/group-orders",
            json!({"title": "x", "rosterId": rosters[0]["id"], "vendorId": vendors[0]["id"]}),
        )
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, v, _) = send(
        &f.c.app,
        "GET",
        &format!("/api/v1/group-orders/{id}"),
        None,
        None,
        &[],
    )
    .await;
    assert_eq!(
        (s, v["code"].as_str()),
        (StatusCode::UNAUTHORIZED, Some("unauthorized"))
    );
}

#[tokio::test]
async fn create_validation() {
    let f = Fixture::new().await;
    let (_, rosters) = f.c.get("/rosters").await;
    let (_, vendors) = f.c.get("/vendors").await;
    let mk = |extra: Value| {
        let mut b =
            json!({"title": "T", "rosterId": rosters[0]["id"], "vendorId": vendors[0]["id"]});
        for (k, v) in extra.as_object().unwrap() {
            b[k] = v.clone();
        }
        b
    };
    let (s, v) = f.c.post("/group-orders", mk(json!({"title": "  "}))).await;
    assert_eq!(
        (s, v["code"].as_str(), v["field"].as_str()),
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            Some("invalid_title"),
            Some("title")
        )
    );
    let (s, v) =
        f.c.post(
            "/group-orders",
            mk(json!({"deadlineAt": "2001-01-01T00:00:00Z"})),
        )
        .await;
    assert_eq!(
        (s, v["code"].as_str()),
        (StatusCode::UNPROCESSABLE_ENTITY, Some("invalid_deadlineAt"))
    );
    let (s, v) =
        f.c.post("/group-orders", mk(json!({"deadlineAt": "tomorrow"})))
            .await;
    assert_eq!(
        (s, v["code"].as_str()),
        (StatusCode::UNPROCESSABLE_ENTITY, Some("invalid_deadlineAt"))
    );
    let (s, v) =
        f.c.post(
            "/group-orders",
            mk(json!({"deadlineAt": "2999-01-01T12:00:00+08:00"})),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED);
    assert_eq!(v["deadlineAt"], "2999-01-01T04:00:00Z");
    // Unknown roster / vendor -> 404.
    let (s, _) =
        f.c.post("/group-orders", mk(json!({"rosterId": "not-a-uuid"})))
            .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn full_ordering_flow() {
    let f = Fixture::new().await;
    let (s, v) = f.public_get("").await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["title"], "週五午餐");
    assert_eq!(v["status"], "open");
    assert_eq!(v["acceptingOrders"], true);
    assert_eq!(v["vendor"]["name"], "好吃便當");
    assert_eq!(v["menu"].as_array().unwrap().len(), 3);
    assert_eq!(
        v["menu"][0],
        json!({"id": f.item("排骨飯"), "name": "排骨飯", "price": 100, "description": null, "category": "飯", "soldOut": false})
    );
    let ps = v["participants"].as_array().unwrap();
    assert_eq!(ps.len(), 3);
    assert!(ps.iter().all(|p| p["hasOrdered"] == false));
    assert!(v.get("shareUrl").is_none() && v["participants"][0].get("sortOrder").is_none());

    // Before submitting: order is null.
    let order_path = format!("/participants/{}/order", f.pid("Amy"));
    let (s, v) = f.public_get(&order_path).await;
    assert_eq!((s, v), (StatusCode::OK, json!({"order": null})));

    // Submit.
    let k1 = key();
    let body = json!({
        "lines": [{"itemId": f.item("排骨飯"), "quantity": 2, "note": "飯少"}, {"itemId": f.item("雞腿飯"), "quantity": 1}],
        "note": "謝謝", "idempotencyKey": k1,
        "client": {"deviceId": "dev-1", "deviceModel": "Pixel 8", "screen": "390x844", "language": "zh-TW", "timezone": "Asia/Taipei"}});
    let (s, v) = f.submit("Amy", body.clone()).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let o = &v["order"];
    assert_eq!(o["cancelled"], false);
    assert_eq!(o["total"], 310);
    assert_eq!(o["note"], "謝謝");
    assert_eq!(
        o["lines"][0],
        json!({"itemId": f.item("排骨飯"), "name": "排骨飯", "quantity": 2, "unitPrice": 100, "subtotal": 200, "note": "飯少"})
    );
    assert_eq!(o["lines"][1]["note"], Value::Null);
    let first_id = o["submissionId"].clone();
    assert!(o["submittedAt"].as_str().unwrap().ends_with('Z'));

    // GET order matches; hasOrdered flips only for Amy.
    let (_, got) = f.public_get(&order_path).await;
    assert_eq!(got, v);
    let (_, pub_) = f.public_get("").await;
    let flags: Vec<_> = pub_["participants"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["hasOrdered"].as_bool().unwrap())
        .collect();
    assert_eq!(flags, [true, false, false]);

    // Replay the same key: 200, same order, no new row.
    let (s, again) = f.submit("Amy", body).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(again["order"]["submissionId"], first_id);
    assert_eq!(submission_count(&f).await, 1);

    // Re-submit different items: the newer one is effective.
    let (s, v2) = f
        .submit(
            "Amy",
            json!({"lines": [{"itemId": f.item("素食"), "quantity": 3}], "idempotencyKey": key()}),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_ne!(v2["order"]["submissionId"], first_id);
    assert_eq!(v2["order"]["total"], 270);
    assert_eq!(v2["order"]["lines"].as_array().unwrap().len(), 1);
    assert_eq!(submission_count(&f).await, 2);
    assert_eq!(f.public_get(&order_path).await.1, v2);

    // Cancel with an empty submission.
    let (s, v3) = f
        .submit("Amy", json!({"lines": [], "idempotencyKey": key()}))
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v3["order"]["cancelled"], true);
    assert_eq!(v3["order"]["total"], 0);
    assert_eq!(v3["order"]["lines"], json!([]));
    let (_, pub_) = f.public_get("").await;
    assert!(
        pub_["participants"]
            .as_array()
            .unwrap()
            .iter()
            .all(|p| p["hasOrdered"] == false)
    );
    assert_eq!(submission_count(&f).await, 3);
}

#[tokio::test]
async fn malformed_and_unknown_tokens_are_indistinguishable() {
    let f = Fixture::new().await;
    let pid = f.pid("Amy");
    let unknown = "A".repeat(43);
    let mut seen = Vec::new();
    for token in ["short", "has space!", "ünïcode", unknown.as_str()] {
        let enc = token.replace(' ', "%20");
        let (s, v, h) = send(
            &f.c.app,
            "GET",
            &format!("/api/v1/public/g/{enc}"),
            None,
            None,
            &[],
        )
        .await;
        assert_eq!(s, StatusCode::NOT_FOUND, "{token}");
        assert_eq!(h[header::CONTENT_TYPE], "application/problem+json");
        assert_eq!(h["x-robots-tag"], "noindex");
        seen.push(v);
        let (s, v, _) = send(
            &f.c.app,
            "POST",
            &format!("/api/v1/public/g/{enc}/participants/{pid}/submissions"),
            Some(json!({"lines": [], "idempotencyKey": key()})),
            None,
            &[],
        )
        .await;
        assert_eq!(s, StatusCode::NOT_FOUND);
        seen.push(v);
    }
    assert!(seen.windows(2).all(|w| w[0] == w[1]), "{seen:?}");
    assert_eq!(seen[0]["code"], "not_found");
    assert!(!seen[0].to_string().contains("short"));
    // Unknown participant (valid token) is a 404 as well.
    let ghost = uuid::Uuid::now_v7();
    assert_eq!(
        f.public_get(&format!("/participants/{ghost}/order"))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        f.public_get("/participants/nope/order").await.0,
        StatusCode::NOT_FOUND
    );
    let (s, _, _) = send(
        &f.c.app,
        "POST",
        &format!("{}/participants/{ghost}/submissions", f.base()),
        Some(json!({"lines": [], "idempotencyKey": key()})),
        None,
        &[],
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    assert_eq!(submission_count(&f).await, 0);
}

#[tokio::test]
async fn submission_errors() {
    let f = Fixture::new().await;
    let item = f.item("排骨飯");
    let line = |q: u32| json!({"itemId": item, "quantity": q});

    // Validation.
    for (body, code) in [
        (
            json!({"lines": [line(0)], "idempotencyKey": key()}),
            "invalid_quantity",
        ),
        (
            json!({"lines": [line(21)], "idempotencyKey": key()}),
            "invalid_quantity",
        ),
        (
            json!({"lines": [line(1)], "idempotencyKey": "nope"}),
            "invalid_idempotencyKey",
        ),
        (
            json!({"lines": vec![line(1); 31], "idempotencyKey": key()}),
            "limit_exceeded",
        ),
        (
            json!({"lines": [{"itemId": item, "quantity": 1, "note": "x".repeat(51)}], "idempotencyKey": key()}),
            "invalid_note",
        ),
    ] {
        let (s, v) = f.submit("Amy", body).await;
        assert_eq!(
            (s, v["code"].as_str()),
            (StatusCode::UNPROCESSABLE_ENTITY, Some(code)),
            "{v}"
        );
    }
    let (s, v) = f.submit("Amy", json!({"idempotencyKey": key()})).await;
    assert_eq!(
        (s, v["code"].as_str()),
        (StatusCode::UNPROCESSABLE_ENTITY, Some("validation_failed"))
    );
    // Unknown item -> 404.
    let (s, _) = f
        .submit("Amy", json!({"lines": [{"itemId": uuid::Uuid::now_v7().to_string(), "quantity": 1}], "idempotencyKey": key()}))
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    assert_eq!(submission_count(&f).await, 0);

    // Sold out -> 409 with field.
    sqlx::query("UPDATE offered_items SET sold_out = 1 WHERE id = ?")
        .bind(&item)
        .execute(&f.state().db.write)
        .await
        .unwrap();
    let (s, v) = f
        .submit("Amy", json!({"lines": [line(1)], "idempotencyKey": key()}))
        .await;
    assert_eq!(s, StatusCode::CONFLICT);
    assert_eq!(
        (v["code"].as_str(), v["field"].as_str()),
        (Some("sold_out"), Some("offeredItemId"))
    );
    let (_, pub_) = f.public_get("").await;
    assert_eq!(pub_["menu"][0]["soldOut"], true);

    // A successful order, then close: replay still answers, new submits are 409.
    let k = key();
    let ok = json!({"lines": [{"itemId": f.item("雞腿飯"), "quantity": 1}], "idempotencyKey": k});
    assert_eq!(f.submit("Amy", ok.clone()).await.0, StatusCode::OK);
    sqlx::query("UPDATE group_orders SET status = 'closed'")
        .execute(&f.state().db.write)
        .await
        .unwrap();
    let (s, v) = f.submit("Amy", ok).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (s, v) = f
        .submit("Amy", json!({"lines": [{"itemId": f.item("雞腿飯"), "quantity": 2}], "idempotencyKey": key()}))
        .await;
    assert_eq!(
        (s, v["code"].as_str()),
        (StatusCode::CONFLICT, Some("group_closed"))
    );
    let (_, pub_) = f.public_get("").await;
    assert_eq!(
        (pub_["status"].as_str(), pub_["acceptingOrders"].as_bool()),
        (Some("closed"), Some(false))
    );
    // Past deadline while still open.
    sqlx::query("UPDATE group_orders SET status = 'open', deadline_at = '2020-01-01T00:00:00Z'")
        .execute(&f.state().db.write)
        .await
        .unwrap();
    let (s, v) = f
        .submit("Bob", json!({"lines": [{"itemId": f.item("雞腿飯"), "quantity": 1}], "idempotencyKey": key()}))
        .await;
    assert_eq!(
        (s, v["code"].as_str()),
        (StatusCode::CONFLICT, Some("deadline_passed"))
    );
    let (_, pub_) = f.public_get("").await;
    assert_eq!(pub_["acceptingOrders"], false);
    assert_eq!(submission_count(&f).await, 1);
}

#[tokio::test]
async fn client_info_extraction_and_xff_trust() {
    let t = test_app_with(
        true,
        false,
        crate::client::parse_trusted_proxies(&["10.0.0.0/8".into()]).unwrap(),
        RateLimitConfig {
            burst: 1_000,
            per_sec: 1_000.0,
        },
    )
    .await;
    let c = client_for(t).await;
    let f = Fixture::from_client(c).await;
    let item = f.item("排骨飯");
    let path = format!("{}/participants/{}/submissions", f.base(), f.pid("Amy"));
    let body = |k: String| {
        json!({"lines": [{"itemId": item, "quantity": 1}], "idempotencyKey": k,
               "client": {"deviceId": "dev-9", "language": "zh-TW"}})
    };

    // Untrusted peer: XFF is ignored; peer IP is recorded.
    let (s, _, _) = send(
        &f.c.app,
        "POST",
        &path,
        Some(body(key())),
        Some("203.0.113.9"),
        &[
            ("x-forwarded-for", "6.6.6.6"),
            ("user-agent", UA_IPHONE_LINE),
        ],
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (ip, summary, device, ua) = last_submission(&f).await;
    assert_eq!(ip.as_deref(), Some("203.0.113.9"));
    assert_eq!(summary.as_deref(), Some("iPhone · iOS 18.1 · LINE 14.17.0"));
    assert_eq!(device.as_deref(), Some("dev-9"));
    assert_eq!(ua.as_deref(), Some(UA_IPHONE_LINE));

    // Trusted peer: right-most untrusted XFF entry (the left one is forgeable).
    let (s, _, _) = send(
        &f.c.app,
        "POST",
        &path,
        Some(body(key())),
        Some("10.0.0.7"),
        &[("x-forwarded-for", "6.6.6.6, 198.51.100.20, 10.0.0.3")],
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (ip, summary, _, ua) = last_submission(&f).await;
    assert_eq!(ip.as_deref(), Some("198.51.100.20"));
    assert_eq!((summary, ua), (None, None));

    // No connect info (should not happen in production): no IP, still works.
    let (s, _, _) = send(
        &f.c.app,
        "POST",
        &path,
        Some(body(key())),
        None,
        &[("x-forwarded-for", "6.6.6.6")],
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(last_submission(&f).await.0, None);
}

async fn client_for(t: TestApp) -> Client {
    let app = router(t.state.clone());
    let cookie = login(&app, "a@x.test").await;
    Client {
        app,
        cookie,
        _t: Some(t),
    }
}

#[tokio::test]
async fn security_headers() {
    let f = Fixture::new().await;
    for (m, p) in [
        ("GET", "/healthz".to_string()),
        ("GET", "/api/v1/me".to_string()),
        ("GET", f.base()),
        ("GET", "/api/v1/public/g/short".to_string()),
        ("GET", "/g/anything".to_string()),
    ] {
        let (_, _, h) = send(&f.c.app, m, &p, None, None, &[]).await;
        assert_eq!(h["referrer-policy"], "no-referrer", "{p}");
        assert_eq!(
            h.contains_key("x-robots-tag"),
            p.starts_with("/api/v1/public/") || p.starts_with("/g/"),
            "{p}"
        );
    }
    let (_, _, h) = send(&f.c.app, "GET", &f.base(), None, None, &[]).await;
    assert_eq!(h["x-robots-tag"], "noindex");
    assert_eq!(h[header::CACHE_CONTROL], "no-store");
}

#[tokio::test]
async fn origin_check_covers_public_posts() {
    let f = Fixture::new().await;
    let path = format!("{}/participants/{}/submissions", f.base(), f.pid("Amy"));
    let body = json!({"lines": [], "idempotencyKey": key()});
    let (s, v, _) = send(
        &f.c.app,
        "POST",
        &path,
        Some(body.clone()),
        None,
        &[("origin", "https://evil.test")],
    )
    .await;
    assert_eq!(
        (s, v["code"].as_str()),
        (StatusCode::FORBIDDEN, Some("bad_origin"))
    );
    let (s, _, _) = send(
        &f.c.app,
        "POST",
        &path,
        Some(body),
        None,
        &[("origin", "http://localhost:3000")],
    )
    .await;
    assert_eq!(s, StatusCode::OK);
}

#[tokio::test]
async fn public_endpoints_are_rate_limited_per_ip() {
    let t = test_app_with(
        true,
        false,
        vec![],
        RateLimitConfig {
            burst: 3,
            per_sec: 0.001,
        },
    )
    .await;
    let f = Fixture::from_client(client_for(t).await).await;
    let get = |peer: &'static str| {
        let app = f.c.app.clone();
        let path = f.base();
        async move { send(&app, "GET", &path, None, Some(peer), &[]).await }
    };
    for _ in 0..3 {
        assert_eq!(get("192.0.2.1").await.0, StatusCode::OK);
    }
    let (s, v, h) = get("192.0.2.1").await;
    assert_eq!(s, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(v["code"], "rate_limited");
    assert_eq!(h[header::CONTENT_TYPE], "application/problem+json");
    assert!(h.contains_key(header::RETRY_AFTER));
    assert_eq!(h["referrer-policy"], "no-referrer");
    // Spoofing XFF does not dodge the limit; another peer has its own bucket.
    let (s, _, _) = send(
        &f.c.app,
        "GET",
        &f.base(),
        None,
        Some("192.0.2.1"),
        &[("x-forwarded-for", "9.9.9.9")],
    )
    .await;
    assert_eq!(s, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(get("192.0.2.2").await.0, StatusCode::OK);
    // Organizer endpoints are not limited.
    for _ in 0..10 {
        assert_eq!(f.c.get("/rosters").await.0, StatusCode::OK);
    }
}

#[test]
fn openapi_documents_the_ordering_endpoints() {
    let doc = serde_json::to_value(crate::openapi()).unwrap();
    let paths = &doc["paths"];
    assert!(paths["/api/v1/group-orders"]["post"].is_object());
    assert!(paths["/api/v1/group-orders/{id}"]["get"].is_object());
    assert!(paths["/api/v1/public/g/{token}"]["get"].is_object());
    assert!(paths["/api/v1/public/g/{token}/participants/{pid}/order"]["get"].is_object());
    assert!(paths["/api/v1/public/g/{token}/participants/{pid}/submissions"]["post"].is_object());
    assert_eq!(
        paths["/api/v1/public/g/{token}"]["get"]["tags"][0],
        "public"
    );
    assert_eq!(
        paths["/api/v1/group-orders"]["post"]["tags"][0],
        "group-orders"
    );
}

async fn page(app: &Router, path: &str) -> (StatusCode, HeaderMap, String) {
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    let res = app
        .clone()
        .oneshot(Request::get(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let (status, headers) = (res.status(), res.headers().clone());
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        headers,
        String::from_utf8_lossy(&bytes).into_owned(),
    )
}

const INDEX: &str = "<html><head><title>x</title></head><body></body></html>";

#[tokio::test]
async fn share_page_injects_og_meta() {
    let f = Fixture::new().await;
    let (_, rosters) = f.c.get("/rosters").await;
    let (_, vendors) = f.c.get("/vendors").await;
    let first = |v: &Value| {
        let a = v.as_array().or_else(|| v["items"].as_array()).unwrap();
        a[0]["id"].clone()
    };
    let (s, g2) = f
        .c
        .post(
            "/group-orders",
            json!({"title": "晚餐\"><script>", "rosterId": first(&rosters), "vendorId": first(&vendors),
                   "deadlineAt": "2099-01-01T00:00:00Z"}),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{g2}");
    let token2 = g2["shareUrl"]
        .as_str()
        .unwrap()
        .rsplit('/')
        .next()
        .unwrap()
        .to_owned();
    let app = crate::router_with_assets(
        f.state().clone(),
        std::sync::Arc::new(crate::web::TestAssets(Some(INDEX))),
    );

    let (s, h, body) = page(&app, &format!("/g/{token2}")).await;
    assert_eq!(s, StatusCode::OK);
    assert!(
        body.contains("content=\"晚餐&quot;&gt;&lt;script&gt;\""),
        "{body}"
    );
    assert!(
        body.contains("content=\"好吃便當 · 截止 1/1 08:00\""),
        "{body}"
    );
    assert!(body.contains("<title>晚餐&quot;&gt;&lt;script&gt; · 訂便當</title>"));
    assert!(!body.contains(&token2) && !body.contains("og:url"));
    assert_eq!(h[header::CACHE_CONTROL], "no-store");
    assert_eq!(h["x-robots-tag"], "noindex");
    assert_eq!(h[header::REFERRER_POLICY], "no-referrer");
    // Trailing slash too; no deadline -> vendor only.
    let (s, _, body) = page(&app, &format!("/g/{}/", f.token)).await;
    assert_eq!(s, StatusCode::OK);
    assert!(body.contains("content=\"好吃便當\""), "{body}");
    assert!(body.contains("週五午餐"));
}

#[tokio::test]
async fn share_page_unknown_and_malformed_are_identical() {
    let f = Fixture::new().await;
    let app = crate::router_with_assets(
        f.state().clone(),
        std::sync::Arc::new(crate::web::TestAssets(Some(INDEX))),
    );
    let unknown = "A".repeat(43);
    let (s1, h1, b1) = page(&app, &format!("/g/{unknown}")).await;
    let (s2, h2, b2) = page(&app, "/g/short").await;
    assert_eq!((s1, s2), (StatusCode::OK, StatusCode::OK));
    assert_eq!(b1, b2);
    assert!(b1.contains("og:title\" content=\"訂便當\""));
    assert!(b1.contains("noindex, nofollow") && !b1.contains("og:description"));
    assert!(!b1.contains(&unknown));
    for h in [h1, h2] {
        assert_eq!(h[header::CACHE_CONTROL], "no-store");
        assert_eq!(h["x-robots-tag"], "noindex");
    }
}

#[tokio::test]
async fn share_page_without_frontend_is_503() {
    let f = Fixture::new().await;
    let app = crate::router_with_assets(
        f.state().clone(),
        std::sync::Arc::new(crate::web::TestAssets(None)),
    );
    let (s, _, _) = page(&app, &format!("/g/{}", f.token)).await;
    assert_eq!(s, StatusCode::SERVICE_UNAVAILABLE);
}
