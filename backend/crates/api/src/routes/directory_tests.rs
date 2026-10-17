//! API tests for `/api/v1/rosters` and `/api/v1/vendors` (in-process, temp SQLite).

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

use crate::router;
use crate::testing::{TestApp, test_app};

pub(super) struct Client {
    pub(super) app: Router,
    pub(super) cookie: String,
    pub(super) _t: Option<TestApp>,
}

pub(super) async fn raw(
    app: &Router,
    req: Request<Body>,
) -> (StatusCode, Value, axum::http::HeaderMap) {
    let res = app.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let headers = res.headers().clone();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let v = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, v, headers)
}

pub(super) async fn login(app: &Router, email: &str) -> String {
    let req = Request::post("/auth/dev-login")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({ "email": email }).to_string()))
        .unwrap();
    let (s, _, h) = raw(app, req).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    h[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned()
}

impl Client {
    pub(super) async fn new() -> Self {
        let t = test_app(true, false).await;
        let app = router(t.state.clone());
        let cookie = login(&app, "a@x.test").await;
        Self {
            app,
            cookie,
            _t: Some(t),
        }
    }

    /// A second organizer sharing the same server/database.
    pub(super) async fn other(&self) -> Client {
        let cookie = login(&self.app, "b@x.test").await;
        Client {
            app: self.app.clone(),
            cookie,
            _t: None,
        }
    }

    pub(super) async fn send(
        &self,
        method: &str,
        path: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut b = Request::builder()
            .method(method)
            .uri(format!("/api/v1{path}"))
            .header(header::COOKIE, &self.cookie);
        let body = match body {
            Some(v) => {
                b = b.header(header::CONTENT_TYPE, "application/json");
                Body::from(v.to_string())
            }
            None => Body::empty(),
        };
        let (s, v, _) = raw(&self.app, b.body(body).unwrap()).await;
        (s, v)
    }

    pub(super) async fn get(&self, p: &str) -> (StatusCode, Value) {
        self.send("GET", p, None).await
    }
    pub(super) async fn post(&self, p: &str, b: Value) -> (StatusCode, Value) {
        self.send("POST", p, Some(b)).await
    }
    pub(super) async fn patch(&self, p: &str, b: Value) -> (StatusCode, Value) {
        self.send("PATCH", p, Some(b)).await
    }
    pub(super) async fn put(&self, p: &str, b: Value) -> (StatusCode, Value) {
        self.send("PUT", p, Some(b)).await
    }
    async fn delete(&self, p: &str) -> (StatusCode, Value) {
        self.send("DELETE", p, None).await
    }
}

fn id(v: &Value) -> String {
    v["id"].as_str().unwrap().to_owned()
}

#[tokio::test]
async fn unauthenticated_requests_get_401() {
    let t = test_app(true, false).await;
    let app = router(t.state.clone());
    for (m, p) in [
        ("GET", "/api/v1/rosters"),
        ("POST", "/api/v1/rosters"),
        ("GET", "/api/v1/rosters/x"),
        ("GET", "/api/v1/vendors"),
        ("DELETE", "/api/v1/vendors/x/menu-items/y"),
    ] {
        let req = Request::builder()
            .method(m)
            .uri(p)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from("{}"))
            .unwrap();
        let (s, v, h) = raw(&app, req).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED, "{m} {p}");
        assert_eq!(v["code"], "unauthorized");
        assert_eq!(h[header::CONTENT_TYPE], "application/problem+json");
    }
}

#[tokio::test]
async fn roster_crud_and_upsert_report() {
    let c = Client::new().await;
    let (s, empty) = c.get("/rosters").await;
    assert_eq!((s, empty), (StatusCode::OK, json!([])));

    let (s, created) = c
        .post(
            "/rosters",
            json!({"name": "午餐組", "members": [{"name": "王小明", "note": "素"}, {"name": "Amy"}]}),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{created}");
    assert_eq!(created["report"]["created"], 2);
    assert_eq!(created["members"].as_array().unwrap().len(), 2);
    let rid = id(&created);

    // Create without members has no report.
    let (s, plain) = c.post("/rosters", json!({"name": "空"})).await;
    assert_eq!(s, StatusCode::CREATED);
    assert!(plain.get("report").is_none());
    assert_eq!(plain["members"], json!([]));

    let (_, list) = c.get("/rosters").await;
    assert_eq!(list.as_array().unwrap().len(), 2);
    let item = list
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == rid.as_str())
        .unwrap();
    assert_eq!(item["memberCount"], 2);
    assert_eq!(item["name"], "午餐組");
    assert!(item["updatedAt"].as_str().unwrap().ends_with('Z'));

    // Upsert: trailing space is the same member (unchanged); fullwidth ＡＭＹ is Amy.
    let (s, up) = c
        .post(
            &format!("/rosters/{rid}/members/upsert"),
            json!({"members": [{"name": "王小明 "}, {"name": "ＡＭＹ"}, {"name": "Bob", "note": "x"}]}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{up}");
    let r = &up["report"];
    assert_eq!(
        (
            r["created"].as_u64(),
            r["updated"].as_u64(),
            r["unchanged"].as_u64()
        ),
        (Some(1), Some(0), Some(2))
    );
    let outs = r["outcomes"].as_array().unwrap();
    assert_eq!(outs[0]["input"], "王小明 ");
    assert_eq!(outs[0]["outcome"], "unchanged");
    assert_eq!(outs[1]["input"], "ＡＭＹ");
    assert_eq!(outs[1]["outcome"], "unchanged");
    assert_eq!(outs[2]["outcome"], "created");
    let members = up["roster"]["members"].as_array().unwrap();
    assert_eq!(members.len(), 3);
    assert_eq!(members[1]["name"], "Amy");
    assert_eq!(outs[1]["memberId"], members[1]["id"]);
    assert_eq!(members[2]["sortOrder"], 2);

    // A note on an existing member is an update; blank never clears.
    let (_, up) = c
        .post(
            &format!("/rosters/{rid}/members/upsert"),
            json!({"members": [{"name": "amy", "note": "不吃辣"}, {"name": "王小明", "note": " "}]}),
        )
        .await;
    assert_eq!(up["report"]["outcomes"][0]["outcome"], "updated");
    assert_eq!(up["report"]["outcomes"][1]["outcome"], "unchanged");
    assert_eq!(up["roster"]["members"][0]["note"], "素");

    // GET detail, PATCH rename, PATCH member (merge-patch), DELETE member.
    let (s, detail) = c.get(&format!("/rosters/{rid}")).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(detail["members"].as_array().unwrap().len(), 3);
    let (s, renamed) = c
        .patch(&format!("/rosters/{rid}"), json!({"name": "晚餐組"}))
        .await;
    assert_eq!(
        (s, renamed["name"].clone()),
        (StatusCode::OK, json!("晚餐組"))
    );

    let mid = id(&detail["members"][0]);
    let (s, p) = c
        .patch(
            &format!("/rosters/{rid}/members/{mid}"),
            json!({"note": null}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{p}");
    assert!(p["members"][0]["note"].is_null());
    assert_eq!(p["members"][0]["name"], "王小明");
    let (_, p) = c
        .patch(
            &format!("/rosters/{rid}/members/{mid}"),
            json!({"name": "王大明"}),
        )
        .await;
    assert_eq!(p["members"][0]["name"], "王大明");
    // Renaming onto an existing NameKey -> 409.
    let (s, conflict) = c
        .patch(
            &format!("/rosters/{rid}/members/{mid}"),
            json!({"name": "ａｍｙ"}),
        )
        .await;
    assert_eq!(s, StatusCode::CONFLICT);
    assert_eq!(conflict["code"], "name_taken");
    assert_eq!(conflict["field"], "name");

    assert_eq!(
        c.delete(&format!("/rosters/{rid}/members/{mid}")).await.0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        c.delete(&format!("/rosters/{rid}/members/{mid}")).await.0,
        StatusCode::NOT_FOUND
    );
    let (_, detail) = c.get(&format!("/rosters/{rid}")).await;
    assert_eq!(detail["members"].as_array().unwrap().len(), 2);

    assert_eq!(
        c.delete(&format!("/rosters/{rid}")).await.0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        c.get(&format!("/rosters/{rid}")).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        c.delete(&format!("/rosters/{rid}")).await.0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn validation_error_shape() {
    let c = Client::new().await;
    let (s, v) = c.post("/rosters", json!({"name": "  "})).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(v["code"], "invalid_name");
    assert_eq!(v["field"], "name");
    assert_eq!(v["status"], 422);

    let (_, r) = c.post("/rosters", json!({"name": "R"})).await;
    let rid = id(&r);
    let (s, v) = c
        .post(
            &format!("/rosters/{rid}/members/upsert"),
            json!({"members": [{"name": "Amy"}, {"name": "ａｍｙ"}]}),
        )
        .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(v["code"], "duplicate_name");
    assert_eq!(v["field"], "name");
    // All-or-nothing: nothing was written.
    let (_, d) = c.get(&format!("/rosters/{rid}")).await;
    assert_eq!(d["members"], json!([]));

    // Limit.
    let many: Vec<Value> = (0..501).map(|i| json!({"name": format!("p{i}")})).collect();
    let (s, v) = c
        .post(
            &format!("/rosters/{rid}/members/upsert"),
            json!({"members": many}),
        )
        .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(v["code"], "limit_exceeded");

    // Bad price, malformed body, wrong type.
    let (_, vd) = c.post("/vendors", json!({"name": "V"})).await;
    let vid = id(&vd);
    let (s, v) = c
        .post(
            &format!("/vendors/{vid}/menu-items/upsert"),
            json!({"items": [{"name": "飯", "price": 0}]}),
        )
        .await;
    assert_eq!(
        (s, v["code"].clone(), v["field"].clone()),
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            json!("invalid_price"),
            json!("price")
        )
    );
    let (s, v) = c.post("/rosters", json!({"name": 5})).await;
    assert_eq!(
        (s, v["code"].clone()),
        (StatusCode::UNPROCESSABLE_ENTITY, json!("validation_failed"))
    );
    let req = Request::post("/api/v1/rosters")
        .header(header::COOKIE, &c.cookie)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from("{nope"))
        .unwrap();
    let (s, v, h) = raw(&c.app, req).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert_eq!(v["code"], "invalid_json");
    assert_eq!(h[header::CONTENT_TYPE], "application/problem+json");
}

#[tokio::test]
async fn vendor_crud_and_name_taken() {
    let c = Client::new().await;
    let (s, v) = c
        .post(
            "/vendors",
            json!({"name": "池上便當", "phone": "02-1234", "address": "台北", "note": "n",
                   "menuItems": [{"name": "排骨飯", "price": 100, "category": "飯"}]}),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    assert_eq!(v["report"]["created"], 1);
    assert_eq!(v["menuItems"][0]["price"], 100);
    assert_eq!(v["menuItems"][0]["archived"], false);
    let vid = id(&v);

    // Same NameKey -> 409 vendor_name_taken + existingId.
    let (s, e) = c.post("/vendors", json!({"name": " 池上便當 "})).await;
    assert_eq!(s, StatusCode::CONFLICT);
    assert_eq!(e["code"], "vendor_name_taken");
    assert_eq!(e["existingId"], vid.as_str());
    let (s, e) = c.post("/vendors", json!({"name": "ＡＢＣ"})).await;
    assert_eq!(s, StatusCode::CREATED, "{e}");
    let abc = id(&e);
    let (s, e) = c.post("/vendors", json!({"name": "abc"})).await;
    assert_eq!(
        (s, e["existingId"].clone()),
        (StatusCode::CONFLICT, json!(abc))
    );

    let (_, list) = c.get("/vendors").await;
    let row = list
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == vid.as_str())
        .unwrap();
    assert_eq!(
        (row["itemCount"].clone(), row["phone"].clone()),
        (json!(1), json!("02-1234"))
    );

    // PATCH: absent keeps, null clears, value sets.
    let (s, p) = c
        .patch(&format!("/vendors/{vid}"), json!({"phone": null}))
        .await;
    assert_eq!(s, StatusCode::OK, "{p}");
    assert!(p["phone"].is_null());
    assert_eq!(p["address"], "台北");
    assert_eq!(p["note"], "n");
    let (_, p) = c
        .patch(
            &format!("/vendors/{vid}"),
            json!({"address": "新北", "note": null}),
        )
        .await;
    assert_eq!(
        (p["address"].clone(), p["note"].clone()),
        (json!("新北"), Value::Null)
    );
    // Rename onto another vendor's key -> 409 with that vendor's id.
    let (s, e) = c
        .patch(&format!("/vendors/{vid}"), json!({"name": "ABC"}))
        .await;
    assert_eq!(s, StatusCode::CONFLICT);
    assert_eq!(e["code"], "vendor_name_taken");
    assert_eq!(e["existingId"], abc.as_str());
    // Renaming to a case variant of itself is fine.
    let (s, p) = c
        .patch(&format!("/vendors/{vid}"), json!({"name": "池上便當 "}))
        .await;
    assert_eq!((s, p["name"].clone()), (StatusCode::OK, json!("池上便當")));
    let (s, e) = c
        .patch(&format!("/vendors/{vid}"), json!({"name": ""}))
        .await;
    assert_eq!(
        (s, e["code"].clone()),
        (StatusCode::UNPROCESSABLE_ENTITY, json!("invalid_name"))
    );

    let (s, d) = c.get(&format!("/vendors/{vid}")).await;
    assert_eq!(s, StatusCode::OK);
    assert!(d["updatedAt"].is_string());
    assert_eq!(
        c.delete(&format!("/vendors/{vid}")).await.0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        c.get(&format!("/vendors/{vid}")).await.0,
        StatusCode::NOT_FOUND
    );
    // The name is free again.
    assert_eq!(
        c.post("/vendors", json!({"name": "池上便當"})).await.0,
        StatusCode::CREATED
    );
}

#[tokio::test]
async fn menu_upsert_price_overwrite_revival_and_patch() {
    let c = Client::new().await;
    let (_, v) = c.post("/vendors", json!({"name": "V"})).await;
    let vid = id(&v);
    let (s, up) = c
        .post(
            &format!("/vendors/{vid}/menu-items/upsert"),
            json!({"items": [{"name": "排骨飯", "price": 100, "description": "招牌"}, {"name": "雞腿飯", "price": 120}]}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{up}");
    assert_eq!(up["report"]["created"], 2);
    let items = up["vendor"]["menuItems"].as_array().unwrap();
    assert_eq!(
        (items[0]["sortOrder"].clone(), items[1]["sortOrder"].clone()),
        (json!(0), json!(1))
    );
    let pid = id(&items[0]);
    assert_eq!(up["report"]["outcomes"][0]["itemId"], pid.as_str());

    // Price overwrites; blank description keeps the old one.
    let (_, up) = c
        .post(
            &format!("/vendors/{vid}/menu-items/upsert"),
            json!({"items": [{"name": " 排骨飯", "price": 110, "description": ""}, {"name": "雞腿飯", "price": 120}]}),
        )
        .await;
    assert_eq!(up["report"]["outcomes"][0]["outcome"], "updated");
    assert_eq!(up["report"]["outcomes"][1]["outcome"], "unchanged");
    assert_eq!(up["vendor"]["menuItems"][0]["price"], 110);
    assert_eq!(up["vendor"]["menuItems"][0]["description"], "招牌");

    // Archive via PATCH, clear description, then upsert revives it.
    let (s, p) = c
        .patch(
            &format!("/vendors/{vid}/menu-items/{pid}"),
            json!({"archived": true, "description": null, "sortOrder": 5, "category": "飯"}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{p}");
    let it = &p["menuItems"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["id"] == pid.as_str())
        .unwrap()
        .clone();
    assert_eq!(it["archived"], true);
    assert!(it["description"].is_null());
    assert_eq!(it["category"], "飯");
    assert_eq!(p["menuItems"][1]["id"], pid.as_str(), "sorted by sortOrder");
    let (_, up) = c
        .post(
            &format!("/vendors/{vid}/menu-items/upsert"),
            json!({"items": [{"name": "排骨飯", "price": 110}]}),
        )
        .await;
    assert_eq!(up["report"]["outcomes"][0]["outcome"], "updated");
    let revived = up["vendor"]["menuItems"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["id"] == pid.as_str())
        .unwrap()
        .clone();
    assert_eq!(revived["archived"], false);

    // Rename collision -> 409; invalid price -> 422.
    let other = id(&up["vendor"]["menuItems"][0]);
    let (s, e) = c
        .patch(
            &format!("/vendors/{vid}/menu-items/{other}"),
            json!({"name": "ＰＡＩ"}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{e}");
    let (s, e) = c
        .patch(
            &format!("/vendors/{vid}/menu-items/{pid}"),
            json!({"name": "pai"}),
        )
        .await;
    assert_eq!(
        (s, e["code"].clone()),
        (StatusCode::CONFLICT, json!("name_taken"))
    );
    let (s, e) = c
        .patch(
            &format!("/vendors/{vid}/menu-items/{pid}"),
            json!({"price": 100001}),
        )
        .await;
    assert_eq!(
        (s, e["code"].clone()),
        (StatusCode::UNPROCESSABLE_ENTITY, json!("invalid_price"))
    );

    assert_eq!(
        c.delete(&format!("/vendors/{vid}/menu-items/{pid}"))
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        c.delete(&format!("/vendors/{vid}/menu-items/{pid}"))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    let (_, d) = c.get(&format!("/vendors/{vid}")).await;
    assert_eq!(d["menuItems"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn other_organizers_resources_are_404() {
    let a = Client::new().await;
    let b = a.other().await;
    let (_, r) = a
        .post(
            "/rosters",
            json!({"name": "R", "members": [{"name": "Amy"}]}),
        )
        .await;
    let rid = id(&r);
    let mid = id(&r["members"][0]);
    let (_, v) = a
        .post(
            "/vendors",
            json!({"name": "V", "menuItems": [{"name": "飯", "price": 50}]}),
        )
        .await;
    let vid = id(&v);
    let iid = id(&v["menuItems"][0]);

    let probes: Vec<(&str, String, Option<Value>)> = vec![
        ("GET", format!("/rosters/{rid}"), None),
        (
            "PATCH",
            format!("/rosters/{rid}"),
            Some(json!({"name": "x"})),
        ),
        ("DELETE", format!("/rosters/{rid}"), None),
        (
            "POST",
            format!("/rosters/{rid}/members/upsert"),
            Some(json!({"members": [{"name": "Z"}]})),
        ),
        (
            "PATCH",
            format!("/rosters/{rid}/members/{mid}"),
            Some(json!({"name": "x"})),
        ),
        ("DELETE", format!("/rosters/{rid}/members/{mid}"), None),
        ("GET", format!("/vendors/{vid}"), None),
        (
            "PATCH",
            format!("/vendors/{vid}"),
            Some(json!({"name": "x"})),
        ),
        ("DELETE", format!("/vendors/{vid}"), None),
        (
            "POST",
            format!("/vendors/{vid}/menu-items/upsert"),
            Some(json!({"items": [{"name": "Z", "price": 1}]})),
        ),
        (
            "PATCH",
            format!("/vendors/{vid}/menu-items/{iid}"),
            Some(json!({"price": 5})),
        ),
        ("DELETE", format!("/vendors/{vid}/menu-items/{iid}"), None),
        ("GET", "/rosters/not-a-uuid".into(), None),
    ];
    for (m, p, body) in probes {
        let (s, v) = b.send(m, &p, body).await;
        assert_eq!(s, StatusCode::NOT_FOUND, "{m} {p}");
        assert_eq!(v["code"], "not_found");
    }
    assert_eq!(b.get("/rosters").await.1, json!([]));
    assert_eq!(b.get("/vendors").await.1, json!([]));
    // Untouched for the owner; and the other organizer may reuse the vendor name.
    let (_, d) = a.get(&format!("/rosters/{rid}")).await;
    assert_eq!(d["name"], "R");
    assert_eq!(d["members"].as_array().unwrap().len(), 1);
    assert_eq!(
        b.post("/vendors", json!({"name": "V"})).await.0,
        StatusCode::CREATED
    );
}

#[tokio::test]
async fn concurrent_member_upserts_via_api_keep_both() {
    let c = Client::new().await;
    let (_, r) = c.post("/rosters", json!({"name": "R"})).await;
    let rid = id(&r);
    let path = format!("/rosters/{rid}/members/upsert");
    let (x, y) = tokio::join!(
        c.post(&path, json!({"members": [{"name": "甲"}]})),
        c.post(&path, json!({"members": [{"name": "乙"}]})),
    );
    assert_eq!((x.0, y.0), (StatusCode::OK, StatusCode::OK));
    let (_, d) = c.get(&format!("/rosters/{rid}")).await;
    assert_eq!(d["members"].as_array().unwrap().len(), 2);
}
