//! API tests for `/api/v1/ai/import-runs` with scripted agents.

use std::sync::Arc;

use application::ai_import::tools::{
    ConfidenceArg, NewLiteral, ProposeVendorArgs, VendorInfoArg, VendorTargetArg,
};
use application::ai_import::{
    AgentInput, AgentOutcome, AiError, FakeImportAgent, FakeToolCall, ImportAgent,
    ImportToolContext,
};
use async_trait::async_trait;
use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use infrastructure::ai::DemoImportAgent;
use serde_json::{Value, json};

use super::directory_tests::{Client, raw};
use crate::RateLimitConfig;
use crate::router;
use crate::testing::{TestApp, test_app, test_app_ai};

const JPEG: &[u8] = &[
    0xFF, 0xD8, 0xFF, 0xE0, 0, 0x10, b'J', b'F', b'I', b'F', 0, 1, 1, 0xFF, 0xD9,
];
const BOUNDARY: &str = "XBOUNDARYX";

fn fake_vendor_agent() -> Arc<dyn ImportAgent> {
    use application::ai_import::tools::MenuItemArg;
    let item = |name: &str, price: Option<i64>| MenuItemArg {
        name: name.into(),
        price,
        description: None,
        category: None,
        confidence: ConfidenceArg::High,
    };
    Arc::new(FakeImportAgent::new(
        vec![FakeToolCall::ProposeVendor(ProposeVendorArgs {
            target: VendorTargetArg::New(NewLiteral::New),
            info: VendorInfoArg {
                name: "阿財便當".into(),
                phone: Some("02-2345-6789".into()),
                address: None,
                note: None,
            },
            menu_items: vec![item("排骨飯", Some(85)), item("雞腿飯", None)],
            warnings: vec![],
        })],
        "已提出阿財便當",
    ))
}

struct Failing(AiError);

#[async_trait]
impl ImportAgent for Failing {
    async fn run(
        &self,
        ctx: Arc<ImportToolContext>,
        _i: AgentInput,
    ) -> Result<AgentOutcome, AiError> {
        ctx.add_usage(domain::ai_import::Usage {
            input_tokens: 700,
            output_tokens: 3,
        });
        ctx.record_tool_call("list_vendors", json!({}));
        Err(self.0.clone())
    }
}

fn multipart(parts: &[(&str, Option<&str>, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    for (name, filename, data) in parts {
        out.extend_from_slice(format!("--{BOUNDARY}\r\n").as_bytes());
        match filename {
            Some(f) => out.extend_from_slice(
                format!(
                    "Content-Disposition: form-data; name=\"{name}\"; filename=\"{f}\"\r\nContent-Type: image/jpeg\r\n\r\n"
                )
                .as_bytes(),
            ),
            None => out.extend_from_slice(
                format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes(),
            ),
        }
        out.extend_from_slice(data);
        out.extend_from_slice(b"\r\n");
    }
    out.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());
    out
}

async fn upload_raw(c: &Client, body: Vec<u8>) -> (StatusCode, Value) {
    let req = Request::post("/api/v1/ai/import-runs")
        .header(header::COOKIE, &c.cookie)
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={BOUNDARY}"),
        )
        .body(Body::from(body))
        .unwrap();
    let (s, v, _) = raw(&c.app, req).await;
    (s, v)
}

/// Upload with exactly the given fields (no default `mode`).
async fn upload_exact(c: &Client, image: &[u8], fields: &[(&str, &str)]) -> (StatusCode, Value) {
    let mut parts: Vec<(&str, Option<&str>, &[u8])> = vec![("image", Some("flyer.jpg"), image)];
    for (k, v) in fields {
        parts.push((k, None, v.as_bytes()));
    }
    upload_raw(c, multipart(&parts)).await
}

/// Upload as a vendor flyer unless `fields` carries a `mode`.
async fn upload(c: &Client, image: &[u8], fields: &[(&str, &str)]) -> (StatusCode, Value) {
    let mut fields = fields.to_vec();
    if !fields.iter().any(|(k, _)| *k == "mode") {
        fields.push(("mode", "vendor"));
    }
    upload_exact(c, image, &fields).await
}

async fn client_with(t: TestApp) -> Client {
    let app: Router = router(t.state.clone());
    let cookie = super::directory_tests::login(&app, "a@x.test").await;
    Client {
        app,
        cookie,
        _t: Some(t),
    }
}

async fn client() -> Client {
    client_with(test_app_ai(fake_vendor_agent(), 30, None, false).await).await
}

#[tokio::test]
async fn delivery_address_import_is_reviewed_before_atomic_apply() {
    let agent = Arc::new(FakeImportAgent::new(
        vec![FakeToolCall::ProposeDeliveryAddress(
            application::ai_import::tools::ProposeDeliveryAddressArgs {
                name: "辨識教室".into(),
                address: "模糊地址".into(),
                confidence: ConfidenceArg::Low,
                warnings: vec!["請核對樓層".into()],
            },
        )],
        "已辨識地址，請確認。",
    ));
    let c = client_with(test_app_ai(agent, 30, None, false).await).await;
    let (s, run) = upload_exact(&c, JPEG, &[("mode", "deliveryAddress")]).await;
    assert_eq!(s, StatusCode::CREATED, "{run}");
    assert_eq!(run["mode"], "deliveryAddress");
    assert_eq!(run["proposal"]["targets"][0]["type"], "deliveryAddress");
    assert!(
        c.get("/delivery-addresses")
            .await
            .1
            .as_array()
            .unwrap()
            .is_empty()
    );
    let path = format!("/ai/import-runs/{}/apply", run["id"].as_str().unwrap());
    assert_eq!(
        c.other().await.post(&path, json!({"accepted":[]})).await.0,
        StatusCode::NOT_FOUND
    );
    let invalid = json!({"accepted":[
        {"type":"deliveryAddress", "name":"有效教室", "address":"有效地址"},
        {"type":"deliveryAddress", "name":"教室", "address":""}
    ]});
    assert_eq!(
        c.post(&path, invalid).await.0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert!(
        c.get("/delivery-addresses")
            .await
            .1
            .as_array()
            .unwrap()
            .is_empty()
    );
    let confirmed = json!({"accepted":[{"type":"deliveryAddress", "name":"台北教室", "address":"確認地址 3 樓"}]});
    let (s, result) = c.post(&path, confirmed.clone()).await;
    assert_eq!(s, StatusCode::OK, "{result}");
    assert_eq!(result["deliveryAddressIds"].as_array().unwrap().len(), 1);
    assert_eq!(
        c.get("/delivery-addresses").await.1[0]["address"],
        "確認地址 3 樓"
    );
    assert_eq!(c.post(&path, confirmed).await.0, StatusCode::CONFLICT);
    assert!(c.get("/vendors").await.1.as_array().unwrap().is_empty());
}

fn accepted_vendor() -> Value {
    json!({ "accepted": [{
        "type": "vendor",
        "vendor": { "kind": "new", "name": "阿財便當" },
        "info": { "name": "阿財便當", "phone": "02-2345-6789", "address": null, "note": null },
        "menu": [
            { "name": "排骨飯", "price": 85, "description": null, "category": null },
            { "name": "雞腿飯", "price": 95, "description": null, "category": null }
        ]
    }]})
}

#[tokio::test]
async fn upload_returns_proposal() {
    let c = client().await;
    let (s, v) = upload(&c, JPEG, &[("mode", "vendor"), ("instruction", "午餐店")]).await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    assert_eq!(v["status"], "proposed");
    assert_eq!(v["mode"], "vendor");
    assert_eq!(v["instruction"], "午餐店");
    assert_eq!(v["summary"], "已提出阿財便當");
    assert_eq!(v["usage"]["inputTokens"], 1000);
    assert!(v["finishedAt"].is_string() && v["appliedAt"].is_null());
    let t = &v["proposal"]["targets"][0];
    assert_eq!(t["type"], "vendor");
    assert_eq!(t["vendor"]["kind"], "new");
    assert_eq!(t["menu"][1]["issues"][0], "price_missing");

    // GET returns the same run
    let id = v["id"].as_str().unwrap();
    let (s, g) = c.get(&format!("/ai/import-runs/{id}")).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(g["id"], v["id"]);
}

#[tokio::test]
async fn unauthenticated_gets_401() {
    let c = client().await;
    let req = Request::post("/api/v1/ai/import-runs")
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={BOUNDARY}"),
        )
        .body(Body::from(multipart(&[("image", Some("a.jpg"), JPEG)])))
        .unwrap();
    let (s, v, _) = raw(&c.app, req).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    assert_eq!(v["code"], "unauthorized");
}

#[tokio::test]
async fn image_validation() {
    let c = client().await;
    // magic bytes decide, not the file name / declared type
    let (s, v) = upload(&c, b"this is not an image at all", &[]).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(v["code"], "invalid_image");
    let (s, v) = upload_raw(&c, multipart(&[("instruction", None, b"x")])).await;
    assert_eq!(
        (s, v["code"].as_str()),
        (StatusCode::UNPROCESSABLE_ENTITY, Some("invalid_image"))
    );

    let png = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0, 0];
    let mut webp = b"RIFF\x10\0\0\0WEBPVP8 ".to_vec();
    webp.extend_from_slice(&[0; 8]);
    for img in [&png[..], &webp[..]] {
        let (s, v) = upload(&c, img, &[]).await;
        assert_eq!(s, StatusCode::CREATED, "{v}");
    }
    // RIFF but not WEBP
    let (s, _) = upload(&c, b"RIFF\x10\0\0\0WAVEfmt ", &[]).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);

    // field validation
    let (s, v) = upload(&c, JPEG, &[("instruction", &"字".repeat(201))]).await;
    assert_eq!(
        (s, v["code"].as_str()),
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            Some("invalid_instruction")
        )
    );
    let (s, v) = upload(&c, JPEG, &[("instruction", &"字".repeat(200))]).await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    // the mode is required and must be roster or vendor
    for fields in [&[][..], &[("mode", "menu")], &[("mode", "")]] {
        let (s, v) = upload_exact(&c, JPEG, fields).await;
        assert_eq!(
            (s, v["code"].as_str(), v["field"].as_str()),
            (
                StatusCode::UNPROCESSABLE_ENTITY,
                Some("invalid_mode"),
                Some("mode")
            ),
            "{fields:?}: {v}"
        );
    }
    // the fake agent here scripts vendor tools, which a roster upload does not have
    let (s, v) = upload(&c, JPEG, &[("mode", "roster")]).await;
    assert_eq!(s, StatusCode::BAD_GATEWAY, "{v}");
    // unknown / foreign targets are 404
    let (s, _) = upload(
        &c,
        JPEG,
        &[("targetVendorId", "00000000-0000-7000-8000-000000000000")],
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = upload(&c, JPEG, &[("targetRosterId", "nope")]).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn oversized_image_is_413() {
    let c = client().await;
    let mut big = JPEG.to_vec();
    big.resize(6 * 1024 * 1024, 0);
    let (s, v) = upload(&c, &big, &[]).await;
    assert_eq!(s, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(v["code"], "image_too_large");
    // just over 5 MB but under the body limit: the handler answers
    let mut over = JPEG.to_vec();
    over.resize(5 * 1024 * 1024 + 1, 0);
    let (s, v) = upload(&c, &over, &[]).await;
    assert_eq!(
        (s, v["code"].as_str()),
        (StatusCode::PAYLOAD_TOO_LARGE, Some("image_too_large"))
    );
    // exactly 5 MB is fine
    let mut ok = JPEG.to_vec();
    ok.resize(5 * 1024 * 1024, 0);
    let (s, _) = upload(&c, &ok, &[]).await;
    assert_eq!(s, StatusCode::CREATED);
}

#[tokio::test]
async fn disabled_is_503_and_config_reports_it() {
    let t = test_app(true, false).await;
    let c = client_with(t).await;
    let (s, v) = upload(&c, JPEG, &[]).await;
    assert_eq!(s, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(v["code"], "ai_not_configured");
    let (s, _) = c.get("/ai/import-runs/abc").await;
    assert_eq!(s, StatusCode::SERVICE_UNAVAILABLE);

    let (s, v) = c.get("/auth/config").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(
        (v["aiImportEnabled"].clone(), v["aiDemo"].clone()),
        (json!(false), json!(false))
    );

    let c =
        client_with(test_app_ai(Arc::new(DemoImportAgent::default()), 30, None, true).await).await;
    let (_, v) = c.get("/auth/config").await;
    assert_eq!(
        (v["aiImportEnabled"].clone(), v["aiDemo"].clone()),
        (json!(true), json!(true))
    );
    let c = client().await;
    let (_, v) = c.get("/auth/config").await;
    assert_eq!(
        (v["aiImportEnabled"].clone(), v["aiDemo"].clone()),
        (json!(true), json!(false))
    );
}

#[tokio::test]
async fn quota_exceeded_is_429() {
    let c = client_with(test_app_ai(fake_vendor_agent(), 2, None, false).await).await;
    assert_eq!(upload(&c, JPEG, &[]).await.0, StatusCode::CREATED);
    assert_eq!(upload(&c, JPEG, &[]).await.0, StatusCode::CREATED);
    let (s, v) = upload(&c, JPEG, &[]).await;
    assert_eq!(s, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(v["code"], "ai_quota_exceeded");
}

#[tokio::test]
async fn rate_limit_is_per_organizer() {
    let rate = RateLimitConfig {
        burst: 3,
        per_sec: 3.0 / 60.0,
    };
    let c = client_with(test_app_ai(fake_vendor_agent(), 30, Some(rate), false).await).await;
    for _ in 0..3 {
        assert_eq!(upload(&c, JPEG, &[]).await.0, StatusCode::CREATED);
    }
    let req = Request::post("/api/v1/ai/import-runs")
        .header(header::COOKIE, &c.cookie)
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={BOUNDARY}"),
        )
        .body(Body::from(multipart(&[("image", Some("a.jpg"), JPEG)])))
        .unwrap();
    let (s, v, h) = raw(&c.app, req).await;
    assert_eq!(s, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(v["code"], "rate_limited");
    assert!(h.contains_key(header::RETRY_AFTER));
    // another organizer has their own bucket
    let other = c.other().await;
    assert_eq!(upload(&other, JPEG, &[]).await.0, StatusCode::CREATED);
}

#[tokio::test]
async fn refusal_is_422_with_run_id_and_failed_run_is_readable() {
    let agent = Arc::new(Failing(AiError::Refused {
        category: Some("cyber".into()),
    }));
    let c = client_with(test_app_ai(agent, 30, None, false).await).await;
    let (s, v) = upload(&c, JPEG, &[]).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(v["code"], "ai_refused");
    let run_id = v["runId"].as_str().expect("runId in problem").to_owned();
    let (s, run) = c.get(&format!("/ai/import-runs/{run_id}")).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(run["status"], "failed");
    assert_eq!(
        run["usage"]["inputTokens"], 700,
        "failed runs keep their usage"
    );
    assert!(run["error"].is_string());
}

#[tokio::test]
async fn unavailable_and_timeout_are_502() {
    for e in [
        AiError::Unavailable {
            retryable: false,
            detail: "secret provider detail".into(),
        },
        AiError::Timeout,
    ] {
        let c = client_with(test_app_ai(Arc::new(Failing(e)), 30, None, false).await).await;
        let (s, v) = upload(&c, JPEG, &[]).await;
        assert_eq!(s, StatusCode::BAD_GATEWAY);
        assert_eq!(v["code"], "ai_unavailable");
        assert!(v["runId"].is_string());
        assert!(!v.to_string().contains("secret provider detail"));
    }
}

#[tokio::test]
async fn apply_writes_vendor_and_second_apply_conflicts() {
    let c = client().await;
    let (_, run) = upload(&c, JPEG, &[("mode", "vendor")]).await;
    let id = run["id"].as_str().unwrap();

    let (s, v) = c
        .post(&format!("/ai/import-runs/{id}/apply"), accepted_vendor())
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["vendorIds"].as_array().unwrap().len(), 1);
    assert_eq!(v["rosterIds"].as_array().unwrap().len(), 0);

    // visible through the directory API
    let (_, vendors) = c.get("/vendors").await;
    let vendor = &vendors.as_array().unwrap()[0];
    assert_eq!(vendor["name"], "阿財便當");
    assert_eq!(vendor["itemCount"], 2);
    let vid = v["vendorIds"][0].as_str().unwrap();
    let (_, detail) = c.get(&format!("/vendors/{vid}")).await;
    let prices: Vec<_> = detail["menuItems"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["price"].as_i64().unwrap())
        .collect();
    assert_eq!(prices, [85, 95]);

    let (_, run) = c.get(&format!("/ai/import-runs/{id}")).await;
    assert_eq!(run["status"], "applied");
    assert!(run["appliedAt"].is_string());

    let (s, v) = c
        .post(&format!("/ai/import-runs/{id}/apply"), accepted_vendor())
        .await;
    assert_eq!(s, StatusCode::CONFLICT);
    assert_eq!(v["code"], "invalid_state");
    let (s, v) = c
        .post(&format!("/ai/import-runs/{id}/discard"), json!({}))
        .await;
    assert_eq!(
        (s, v["code"].as_str()),
        (StatusCode::CONFLICT, Some("invalid_state"))
    );
}

#[tokio::test]
async fn apply_with_invalid_changes_lists_errors() {
    let c = client().await;
    let (_, run) = upload(&c, JPEG, &[]).await;
    let id = run["id"].as_str().unwrap();
    let mut body = accepted_vendor();
    body["accepted"][0]["menu"][1]["price"] = Value::Null;
    let (s, v) = c.post(&format!("/ai/import-runs/{id}/apply"), body).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "{v}");
    assert_eq!(v["code"], "invalid_changes");
    let errors = v["errors"].as_array().unwrap();
    assert_eq!(errors[0]["path"], "accepted[0].menu[1].price");
    assert!(errors[0]["message"].is_string());
    // nothing was written and the run can still be applied
    assert_eq!(c.get("/vendors").await.1.as_array().unwrap().len(), 0);
    let (s, _) = c
        .post(&format!("/ai/import-runs/{id}/apply"), accepted_vendor())
        .await;
    assert_eq!(s, StatusCode::OK);
}

#[tokio::test]
async fn discard_returns_204() {
    let c = client().await;
    let (_, run) = upload(&c, JPEG, &[]).await;
    let id = run["id"].as_str().unwrap();
    let (s, _) = c
        .post(&format!("/ai/import-runs/{id}/discard"), json!({}))
        .await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (_, run) = c.get(&format!("/ai/import-runs/{id}")).await;
    assert_eq!(run["status"], "discarded");
    assert_eq!(c.get("/vendors").await.1.as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn other_organizers_runs_are_404() {
    let c = client().await;
    let (_, run) = upload(&c, JPEG, &[]).await;
    let id = run["id"].as_str().unwrap();
    let other = c.other().await;
    assert_eq!(
        other.get(&format!("/ai/import-runs/{id}")).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        other
            .post(&format!("/ai/import-runs/{id}/apply"), accepted_vendor())
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        other
            .post(&format!("/ai/import-runs/{id}/discard"), json!({}))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        c.get(&format!("/ai/import-runs/{id}")).await.0,
        StatusCode::OK
    );
}

fn demo_delay0() -> Arc<dyn ImportAgent> {
    Arc::new(DemoImportAgent {
        delay: std::time::Duration::ZERO,
    })
}

#[tokio::test]
async fn demo_agent_roster_and_vendor() {
    let c = client_with(test_app_ai(demo_delay0(), 30, None, true).await).await;
    let (s, v) = upload(&c, JPEG, &[("mode", "roster")]).await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    let t = &v["proposal"]["targets"][0];
    assert_eq!(t["type"], "roster");
    assert_eq!(t["members"].as_array().unwrap().len(), 4);
    assert!(
        t["members"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["confidence"] == "low")
    );
    assert!(
        t["members"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["note"].is_string())
    );
    assert_eq!(v["model"], "demo");
    assert!(v["summary"].as_str().unwrap().contains("示範"));

    // vendor demo against an existing vendor yields a price change
    let (_, vendor) = c.post("/vendors", json!({ "name": "老王便當" })).await;
    let vid = vendor["id"].as_str().unwrap();
    c.post(
        &format!("/vendors/{vid}/menu-items/upsert"),
        json!({ "items": [{ "name": "排骨飯", "price": 80 }] }),
    )
    .await;
    let (s, v) = upload(&c, JPEG, &[("mode", "vendor"), ("targetVendorId", vid)]).await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    let t = &v["proposal"]["targets"][0];
    assert_eq!(t["vendor"]["existingId"], vid);
    let menu = t["menu"].as_array().unwrap();
    assert_eq!(menu.len(), 4);
    assert_eq!(menu[0]["before"]["price"], 80);
    assert_eq!(menu[0]["price"], 85);
    assert!(menu.iter().any(|m| m["issues"][0] == "price_missing"));

    let (s, v) = upload(&c, JPEG, &[("mode", "vendor")]).await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    assert_eq!(
        v["proposal"]["targets"][0]["info"]["after"]["name"],
        "示範便當"
    );
}

/// The mirror DTOs document exactly what the domain serializes.
#[test]
fn dtos_match_domain_json() {
    use super::ai_import::{AcceptedChangeDto, ProposalDto};
    use application::ai_import::{
        AcceptedChange, AcceptedMember, AcceptedMenuItem, AcceptedRoster, AcceptedVendor,
        AcceptedVendorInfo,
    };
    use domain::ai_import::*;

    let snap = |n: &str| VendorInfoSnapshot {
        name: n.into(),
        phone: None,
        address: None,
        note: None,
    };
    let proposal = Proposal {
        targets: vec![
            ProposalTarget::DeliveryAddress(ProposedDeliveryAddress {
                name: "教室".into(),
                address: "送餐地址 3 樓".into(),
                confidence: Confidence::Medium,
            }),
            ProposalTarget::Roster(RosterTarget {
                roster: TargetRef::New { name: "R".into() },
                members: vec![ProposedMember {
                    action: ChangeAction::Create,
                    name: "A".into(),
                    before_note: None,
                    note: Some("n".into()),
                    confidence: Confidence::Low,
                    issues: vec![Issue::DuplicateInInput],
                }],
            }),
            ProposalTarget::Vendor(VendorTarget {
                vendor: TargetRef::Existing {
                    existing_id: domain::directory::VendorId::new(),
                },
                info: ProposedVendorInfo {
                    action: ChangeAction::Update,
                    before: Some(snap("a")),
                    after: snap("b"),
                },
                menu: vec![ProposedMenuItem {
                    action: ChangeAction::Update,
                    name: "x".into(),
                    before: Some(MenuItemSnapshot {
                        price: 1,
                        description: None,
                        category: None,
                    }),
                    price: None,
                    description: None,
                    category: None,
                    confidence: Confidence::High,
                    issues: vec![Issue::PriceMissing],
                }],
            }),
        ],
        warnings: vec!["w".into()],
    };
    let v = serde_json::to_value(&proposal).unwrap();
    let dto: ProposalDto =
        serde_json::from_value(v.clone()).expect("ProposalDto parses domain JSON");
    assert_eq!(serde_json::to_value(&dto).unwrap(), v);

    let accepted = vec![
        AcceptedChange::DeliveryAddress(application::ai_import::AcceptedDeliveryAddress {
            name: "教室".into(),
            address: "送餐地址 3 樓".into(),
        }),
        AcceptedChange::Roster(AcceptedRoster {
            roster: TargetRef::New { name: "R".into() },
            members: vec![AcceptedMember {
                name: "A".into(),
                note: Some("n".into()),
            }],
        }),
        AcceptedChange::Vendor(AcceptedVendor {
            vendor: TargetRef::Existing {
                existing_id: domain::directory::VendorId::new(),
            },
            info: Some(AcceptedVendorInfo {
                name: Some("b".into()),
                ..Default::default()
            }),
            menu: vec![AcceptedMenuItem {
                name: "x".into(),
                price: Some(1),
                description: None,
                category: None,
            }],
        }),
    ];
    let v = serde_json::to_value(&accepted).unwrap();
    let dtos: Vec<AcceptedChangeDto> =
        serde_json::from_value(v.clone()).expect("AcceptedChangeDto parses");
    assert_eq!(serde_json::to_value(&dtos).unwrap(), v);
}
