//! API tests for the organizer management endpoints of `/api/v1/group-orders`.

use axum::http::StatusCode;
use serde_json::{Value, json};

use super::ordering_tests::{Fixture, key, send};

fn gid(f: &Fixture) -> String {
    f.group["id"].as_str().unwrap().to_owned()
}

#[tokio::test]
async fn delivery_address_snapshot_survives_edits_and_deletion_and_can_be_replaced() {
    let f = Fixture::new().await;
    let other = f.c.other().await;
    let (_, a) =
        f.c.post(
            "/delivery-addresses",
            json!({"name":"台北教室", "address":"舊地址 3 樓"}),
        )
        .await;
    let (_, foreign) = other
        .post(
            "/delivery-addresses",
            json!({"name":"他人教室", "address":"不可讀取"}),
        )
        .await;
    let (_, rosters) = f.c.get("/rosters").await;
    let (_, vendors) = f.c.get("/vendors").await;
    let body = json!({"title":"送餐團", "rosterId":rosters[0]["id"], "vendorId":vendors[0]["id"], "deliveryAddressId":a["id"]});
    let (s, g) = f.c.post("/group-orders", body.clone()).await;
    assert_eq!(s, StatusCode::CREATED, "{g}");
    assert_eq!(g["deliveryAddress"], "台北教室（舊地址 3 樓）");
    let path = format!("/group-orders/{}", g["id"].as_str().unwrap());
    let mut foreign_body = body;
    foreign_body["deliveryAddressId"] = foreign["id"].clone();
    assert_eq!(
        f.c.post("/group-orders", foreign_body).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        f.c.patch(&path, json!({"deliveryAddressId":foreign["id"]}))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    let address_path = format!("/delivery-addresses/{}", a["id"].as_str().unwrap());
    assert_eq!(
        f.c.put(
            &address_path,
            json!({"name":"新教室", "address":"新地址 5 樓"})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        f.c.get(&path).await.1["deliveryAddress"],
        "台北教室（舊地址 3 樓）"
    );
    assert_eq!(
        f.c.patch(&path, json!({"deliveryAddressId":a["id"]}))
            .await
            .1["deliveryAddress"],
        "新教室（新地址 5 樓）"
    );
    assert_eq!(
        f.c.send("DELETE", &address_path, None).await.0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        f.c.get(&path).await.1["deliveryAddress"],
        "新教室（新地址 5 樓）"
    );
    assert!(
        f.c.patch(&path, json!({"deliveryAddressId":null})).await.1["deliveryAddress"].is_null()
    );
}

fn lines(f: &Fixture, items: &[(&str, u32)]) -> Value {
    Value::Array(
        items
            .iter()
            .map(|(n, q)| json!({"itemId": f.item(n), "quantity": q}))
            .collect(),
    )
}

async fn order(f: &Fixture, who: &str, items: &[(&str, u32)]) -> (StatusCode, Value) {
    f.submit(
        who,
        json!({"lines": lines(f, items), "idempotencyKey": key()}),
    )
    .await
}

async fn patch_status(f: &Fixture, status: &str) -> (StatusCode, Value) {
    f.c.patch(
        &format!("/group-orders/{}", gid(f)),
        json!({"status": status}),
    )
    .await
}

async fn participants(f: &Fixture) -> Value {
    let (s, v) =
        f.c.get(&format!("/group-orders/{}/participants", gid(f)))
            .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    v
}

fn row<'a>(v: &'a Value, name: &str) -> &'a Value {
    v.as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == name)
        .unwrap()
}

#[tokio::test]
async fn list_counts_totals_status_filter_and_scoping() {
    let f = Fixture::new().await;
    // A second group from the same master data.
    let (_, rosters) = f.c.get("/rosters").await;
    let (_, vendors) = f.c.get("/vendors").await;
    let (s, g2) =
        f.c.post(
            "/group-orders",
            json!({"title": "週六", "rosterId": rosters[0]["id"], "vendorId": vendors[0]["id"]}),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED);
    let g2id = g2["id"].as_str().unwrap().to_owned();

    assert_eq!(order(&f, "Amy", &[("排骨飯", 2)]).await.0, StatusCode::OK); // 200
    assert_eq!(order(&f, "Bob", &[("雞腿飯", 1)]).await.0, StatusCode::OK); // 110
    // Cat orders then cancels: not counted.
    assert_eq!(order(&f, "Cat", &[("素食", 1)]).await.0, StatusCode::OK);
    assert_eq!(order(&f, "Cat", &[]).await.0, StatusCode::OK);

    let (s, list) = f.c.get("/group-orders").await;
    assert_eq!(s, StatusCode::OK, "{list}");
    let list = list.as_array().unwrap();
    assert_eq!(list.len(), 2);
    // Newest first.
    assert_eq!(list[0]["id"], g2["id"]);
    assert_eq!(list[0]["participantCount"], 3);
    assert_eq!(list[0]["orderedCount"], 0);
    assert_eq!(list[0]["totalAmount"], 0);
    assert_eq!(list[1]["id"], f.group["id"]);
    assert_eq!(list[1]["title"], "週五午餐");
    assert_eq!(list[1]["status"], "open");
    assert_eq!(list[1]["vendorName"], "好吃便當");
    assert_eq!(list[1]["participantCount"], 3);
    assert_eq!(list[1]["orderedCount"], 2);
    assert_eq!(list[1]["totalAmount"], 310);
    assert_eq!(list[1]["closedAt"], Value::Null);
    assert!(list[1]["createdAt"].is_string());

    // Status filter.
    assert_eq!(patch_status(&f, "closed").await.0, StatusCode::OK);
    let (_, open) = f.c.get("/group-orders?status=open").await;
    assert_eq!(open.as_array().unwrap().len(), 1);
    assert_eq!(open[0]["id"], g2["id"]);
    let (_, closed) = f.c.get("/group-orders?status=closed").await;
    assert_eq!(closed.as_array().unwrap().len(), 1);
    assert_eq!(closed[0]["id"], f.group["id"]);
    assert!(closed[0]["closedAt"].is_string());
    let (_, archived) = f.c.get("/group-orders?status=archived").await;
    assert!(archived.as_array().unwrap().is_empty());
    let (s, v) = f.c.get("/group-orders?status=bogus").await;
    assert_eq!(
        (s, v["code"].as_str()),
        (StatusCode::UNPROCESSABLE_ENTITY, Some("invalid_status"))
    );
    // Archived groups are included without a filter.
    assert_eq!(patch_status(&f, "archived").await.0, StatusCode::OK);
    assert_eq!(
        f.c.get("/group-orders").await.1.as_array().unwrap().len(),
        2
    );

    // Another organizer sees nothing.
    let other = f.c.other().await;
    let (s, v) = other.get("/group-orders").await;
    assert_eq!((s, v), (StatusCode::OK, json!([])));
    let _ = g2id;
}

#[tokio::test]
async fn status_state_machine_and_public_effect() {
    let f = Fixture::new().await;
    assert_eq!(order(&f, "Amy", &[("排骨飯", 1)]).await.0, StatusCode::OK);

    // open -> archived is illegal.
    let (s, v) = patch_status(&f, "archived").await;
    assert_eq!(
        (s, v["code"].as_str()),
        (StatusCode::UNPROCESSABLE_ENTITY, Some("invalid_status"))
    );
    // Unknown status value.
    let (s, _) = patch_status(&f, "paused").await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);

    // open -> closed.
    let (s, v) = patch_status(&f, "closed").await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["status"], "closed");
    assert!(v["closedAt"].is_string());
    // Public submit is rejected immediately; the public page says closed.
    let (s, v) = order(&f, "Bob", &[("雞腿飯", 1)]).await;
    assert_eq!(
        (s, v["code"].as_str()),
        (StatusCode::CONFLICT, Some("group_closed"))
    );
    let (_, pg) = f.public_get("").await;
    assert_eq!(
        (pg["status"].as_str(), pg["acceptingOrders"].as_bool()),
        (Some("closed"), Some(false))
    );

    // Organizer can still submit while closed.
    let path = format!(
        "/group-orders/{}/participants/{}/submissions",
        gid(&f),
        f.pid("Bob")
    );
    let (s, v) =
        f.c.post(
            &path,
            json!({"lines": lines(&f, &[("雞腿飯", 2)]), "idempotencyKey": key()}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["order"]["total"], 220);

    // closed -> closed is a no-op; closed -> open -> closed again.
    assert_eq!(patch_status(&f, "closed").await.0, StatusCode::OK);
    let (s, v) = patch_status(&f, "open").await;
    assert_eq!(
        (s, v["status"].as_str(), v["closedAt"].clone()),
        (StatusCode::OK, Some("open"), Value::Null)
    );
    assert_eq!(order(&f, "Bob", &[("雞腿飯", 1)]).await.0, StatusCode::OK);
    assert_eq!(patch_status(&f, "closed").await.1["status"], "closed");

    // closed -> archived; then everything is refused.
    let (s, v) = patch_status(&f, "archived").await;
    assert_eq!(
        (s, v["status"].as_str()),
        (StatusCode::OK, Some("archived"))
    );
    let (s, v) = patch_status(&f, "open").await;
    assert_eq!(
        (s, v["code"].as_str()),
        (StatusCode::UNPROCESSABLE_ENTITY, Some("invalid_status"))
    );
    let (s, v) = order(&f, "Cat", &[("素食", 1)]).await;
    assert_eq!(
        (s, v["code"].as_str()),
        (StatusCode::CONFLICT, Some("group_closed"))
    );
    let (s, v) =
        f.c.post(
            &path,
            json!({"lines": lines(&f, &[("雞腿飯", 1)]), "idempotencyKey": key()}),
        )
        .await;
    assert_eq!(
        (s, v["code"].as_str()),
        (StatusCode::CONFLICT, Some("group_archived"))
    );
}

#[tokio::test]
async fn patch_title_deadline_note_and_scoping() {
    let f = Fixture::new().await;
    let path = format!("/group-orders/{}", gid(&f));
    let (s, v) =
        f.c.patch(
            &path,
            json!({"title": "  新標題 ", "deadlineAt": "2999-01-01T12:00:00+08:00"}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["title"], "新標題");
    assert_eq!(v["deadlineAt"], "2999-01-01T04:00:00Z");
    assert_eq!(v["note"], "11:30 前"); // untouched
    // Clear the deadline with null; replace / clear the note.
    let (_, v) =
        f.c.patch(&path, json!({"deadlineAt": null, "note": "改備註"}))
            .await;
    assert_eq!(
        (v["deadlineAt"].clone(), v["note"].as_str()),
        (Value::Null, Some("改備註"))
    );
    let (_, v) = f.c.patch(&path, json!({"note": null})).await;
    assert_eq!(v["note"], Value::Null);
    assert_eq!(v["title"], "新標題");
    // Validation.
    let (s, v) = f.c.patch(&path, json!({"title": "  "})).await;
    assert_eq!(
        (s, v["code"].as_str()),
        (StatusCode::UNPROCESSABLE_ENTITY, Some("invalid_title"))
    );
    let (s, v) =
        f.c.patch(&path, json!({"deadlineAt": "2001-01-01T00:00:00Z"}))
            .await;
    assert_eq!(
        (s, v["code"].as_str()),
        (StatusCode::UNPROCESSABLE_ENTITY, Some("invalid_deadlineAt"))
    );
    let (s, v) = f.c.patch(&path, json!({"deadlineAt": "soon"})).await;
    assert_eq!(
        (s, v["code"].as_str()),
        (StatusCode::UNPROCESSABLE_ENTITY, Some("invalid_deadlineAt"))
    );
    // Persisted.
    let (_, got) = f.c.get(&path).await;
    assert_eq!(got["title"], "新標題");

    // Every management endpoint is 404 for another organizer and 401 without a session.
    let other = f.c.other().await;
    let id = gid(&f);
    let (pid, item) = (f.pid("Amy"), f.item("排骨飯"));
    let calls: Vec<(&str, String, Option<Value>)> = vec![
        (
            "PATCH",
            format!("/group-orders/{id}"),
            Some(json!({"title": "x"})),
        ),
        (
            "POST",
            format!("/group-orders/{id}/share-token/rotate"),
            None,
        ),
        (
            "POST",
            format!("/group-orders/{id}/participants"),
            Some(json!({"name": "Zed"})),
        ),
        (
            "PATCH",
            format!("/group-orders/{id}/offered-items/{item}"),
            Some(json!({"soldOut": true})),
        ),
        ("GET", format!("/group-orders/{id}/summary"), None),
        ("GET", format!("/group-orders/{id}/participants"), None),
        (
            "GET",
            format!("/group-orders/{id}/participants/{pid}/submissions"),
            None,
        ),
        (
            "POST",
            format!("/group-orders/{id}/participants/{pid}/submissions"),
            Some(json!({"lines": [], "idempotencyKey": key()})),
        ),
        (
            "PUT",
            format!("/group-orders/{id}/participants/{pid}/settlement"),
            Some(json!({"pickedUp": true, "paid": true})),
        ),
    ];
    for (m, p, body) in calls {
        let (s, _) = other.send(m, &p, body.clone()).await;
        assert_eq!(s, StatusCode::NOT_FOUND, "{m} {p}");
        let (s, v, _) = send(&f.c.app, m, &format!("/api/v1{p}"), body, None, &[]).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED, "{m} {p} {v}");
    }
    // Nothing was changed by the foreign attempts.
    assert!(
        !f.public_get("").await.1["participants"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["name"] == "Zed")
    );
    // Unknown participant / item of my own group.
    let (s, _) =
        f.c.get(&format!(
            "/group-orders/{id}/participants/{}/submissions",
            uuid::Uuid::now_v7()
        ))
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) =
        f.c.put(
            &format!(
                "/group-orders/{id}/participants/{}/settlement",
                uuid::Uuid::now_v7()
            ),
            json!({"pickedUp": true, "paid": false}),
        )
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) =
        f.c.patch(
            &format!("/group-orders/{id}/offered-items/{}", uuid::Uuid::now_v7()),
            json!({"soldOut": true}),
        )
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn rotate_share_token_invalidates_the_old_link() {
    let f = Fixture::new().await;
    assert_eq!(f.public_get("").await.0, StatusCode::OK);
    let (s, v) =
        f.c.post(
            &format!("/group-orders/{}/share-token/rotate", gid(&f)),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let url = v["shareUrl"].as_str().unwrap();
    let new_token = url.strip_prefix("http://localhost:3000/g/").unwrap();
    assert_eq!(new_token.len(), 43);
    assert_ne!(new_token, f.token);
    // Old token: 404 on every public endpoint.
    let (s, _) = f.public_get("").await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = f
        .public_get(&format!("/participants/{}/order", f.pid("Amy")))
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = order(&f, "Amy", &[("排骨飯", 1)]).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    // New token works.
    let (s, v, _) = send(
        &f.c.app,
        "GET",
        &format!("/api/v1/public/g/{new_token}"),
        None,
        None,
        &[],
    )
    .await;
    assert_eq!((s, v["title"].as_str()), (StatusCode::OK, Some("週五午餐")));
    // The organizer detail shows the new url.
    let (_, d) = f.c.get(&format!("/group-orders/{}", gid(&f))).await;
    assert_eq!(d["shareUrl"], url);
}

#[tokio::test]
async fn add_participant_and_sold_out() {
    let f = Fixture::new().await;
    let id = gid(&f);
    let (s, v) =
        f.c.post(
            &format!("/group-orders/{id}/participants"),
            json!({"name": "  Dan "}),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    let ps = v["participants"].as_array().unwrap();
    assert_eq!(ps.len(), 4);
    assert_eq!(ps[3]["name"], "Dan");
    assert_eq!(ps[3]["sortOrder"], 3);
    // Visible on the public page and can order (via refreshed group).
    let (_, pg) = f.public_get("").await;
    let dan = pg["participants"][3]["id"].as_str().unwrap().to_owned();
    assert_eq!(pg["participants"][3]["name"], "Dan");
    let (s, _, _) = send(
        &f.c.app,
        "POST",
        &format!("{}/participants/{dan}/submissions", f.base()),
        Some(json!({"lines": lines(&f, &[("素食", 1)]), "idempotencyKey": key()})),
        None,
        &[],
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    // Name collisions (NameKey: case / width insensitive).
    for name in ["dan", "Amy", "ＡＭＹ"] {
        let (s, v) =
            f.c.post(
                &format!("/group-orders/{id}/participants"),
                json!({"name": name}),
            )
            .await;
        assert_eq!(
            (s, v["code"].as_str()),
            (StatusCode::CONFLICT, Some("name_taken")),
            "{name}"
        );
    }
    let (s, v) =
        f.c.post(
            &format!("/group-orders/{id}/participants"),
            json!({"name": "  "}),
        )
        .await;
    assert_eq!(
        (s, v["code"].as_str()),
        (StatusCode::UNPROCESSABLE_ENTITY, Some("invalid_name"))
    );

    // Sold out toggle.
    let item = f.item("雞腿飯");
    let path = format!("/group-orders/{id}/offered-items/{item}");
    let (s, v) = f.c.patch(&path, json!({"soldOut": true})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let it = v["offeredItems"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["id"] == item.as_str())
        .unwrap()
        .clone();
    assert_eq!(it["soldOut"], true);
    let (_, pg) = f.public_get("").await;
    let menu = pg["menu"].as_array().unwrap();
    assert_eq!(
        menu.iter().find(|i| i["id"] == item.as_str()).unwrap()["soldOut"],
        true
    );
    let (s, v) = order(&f, "Amy", &[("雞腿飯", 1)]).await;
    assert_eq!(
        (s, v["code"].as_str()),
        (StatusCode::CONFLICT, Some("sold_out"))
    );
    // Back on sale.
    let (_, v) = f.c.patch(&path, json!({"soldOut": false})).await;
    assert!(
        v["offeredItems"]
            .as_array()
            .unwrap()
            .iter()
            .all(|i| i["soldOut"] == false)
    );
    assert_eq!(order(&f, "Amy", &[("雞腿飯", 1)]).await.0, StatusCode::OK);
}

#[tokio::test]
async fn summary_counts_only_effective_orders_with_stored_prices() {
    let f = Fixture::new().await;
    let id = gid(&f);
    // A: 2x 排骨飯 ; B: 1x 雞腿飯 then 2x 雞腿飯 ; C: orders then cancels.
    assert_eq!(order(&f, "Amy", &[("排骨飯", 2)]).await.0, StatusCode::OK);
    assert_eq!(order(&f, "Bob", &[("雞腿飯", 1)]).await.0, StatusCode::OK);
    assert_eq!(order(&f, "Bob", &[("雞腿飯", 2)]).await.0, StatusCode::OK);
    assert_eq!(order(&f, "Cat", &[("素食", 3)]).await.0, StatusCode::OK);
    assert_eq!(order(&f, "Cat", &[]).await.0, StatusCode::OK);

    // The vendor master changes price after opening: totals must not move.
    let (_, vendors) = f.c.get("/vendors").await;
    let vid = vendors[0]["id"].as_str().unwrap();
    let (_, vd) = f.c.get(&format!("/vendors/{vid}")).await;
    let mid = vd["menuItems"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["name"] == "排骨飯")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let (s, _) =
        f.c.patch(
            &format!("/vendors/{vid}/menu-items/{mid}"),
            json!({"price": 999}),
        )
        .await;
    assert_eq!(s, StatusCode::OK);

    let (s, v) = f.c.get(&format!("/group-orders/{id}/summary")).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(
        v,
        json!({
            "groupOrderId": id,
            "status": "open",
            "items": [
                {"itemId": f.item("排骨飯"), "name": "排骨飯", "price": 100, "quantity": 2, "subtotal": 200},
                {"itemId": f.item("雞腿飯"), "name": "雞腿飯", "price": 110, "quantity": 2, "subtotal": 220},
                {"itemId": f.item("素食"), "name": "素食", "price": 90, "quantity": 0, "subtotal": 0}
            ],
            "totalQuantity": 4,
            "totalAmount": 420,
            "participants": {"total": 3, "ordered": 2, "notOrdered": 1},
            "settlement": {"pickedUp": 0, "paid": 0, "paidAmount": 0, "unpaidAmount": 420}
        })
    );
    // The group's snapshot price (100) is still used for new submissions.
    let path = format!(
        "/group-orders/{id}/participants/{}/submissions",
        f.pid("Amy")
    );
    let (_, v) =
        f.c.post(
            &path,
            json!({"lines": lines(&f, &[("排骨飯", 1)]), "idempotencyKey": key()}),
        )
        .await;
    assert_eq!(v["order"]["lines"][0]["unitPrice"], 100);
}

#[tokio::test]
async fn settlement_flow_balance_and_idempotency() {
    let f = Fixture::new().await;
    let id = gid(&f);
    let amy = f.pid("Amy");
    let path = format!("/group-orders/{id}/participants/{amy}/settlement");
    assert_eq!(order(&f, "Amy", &[("排骨飯", 2)]).await.0, StatusCode::OK); // 200
    assert_eq!(order(&f, "Bob", &[("雞腿飯", 1)]).await.0, StatusCode::OK); // 110

    // Initially unpaid: balance null.
    let ps = participants(&f).await;
    let a = row(&ps, "Amy");
    assert_eq!(
        (
            a["paid"].clone(),
            a["balance"].clone(),
            a["paidAmount"].clone()
        ),
        (json!(false), Value::Null, Value::Null)
    );
    assert_eq!(a["amountDue"], 200);

    // Mark A picked up and paid.
    let (s, v) =
        f.c.put(&path, json!({"pickedUp": true, "paid": true}))
            .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(
        (
            v["pickedUp"].clone(),
            v["paid"].clone(),
            v["paidAmount"].clone(),
            v["balance"].clone()
        ),
        (json!(true), json!(true), json!(200), json!(0))
    );
    let (paid_at, picked_at) = (v["paidAt"].clone(), v["pickedUpAt"].clone());
    assert!(paid_at.is_string() && picked_at.is_string());

    // Idempotent: second identical PUT changes nothing (timestamps kept).
    tokio::time::sleep(std::time::Duration::from_millis(15)).await;
    let (s, v2) =
        f.c.put(&path, json!({"pickedUp": true, "paid": true}))
            .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v2, v);

    let (_, sum) = f.c.get(&format!("/group-orders/{id}/summary")).await;
    assert_eq!(
        sum["settlement"],
        json!({"pickedUp": 1, "paid": 1, "paidAmount": 200, "unpaidAmount": 110})
    );

    // A resubmits more (3x -> 300): 尚差 100. paidAmount stays 200.
    assert_eq!(order(&f, "Amy", &[("排骨飯", 3)]).await.0, StatusCode::OK);
    let ps = participants(&f).await;
    let a = row(&ps, "Amy");
    assert_eq!(
        (
            a["amountDue"].clone(),
            a["paidAmount"].clone(),
            a["balance"].clone()
        ),
        (json!(300), json!(200), json!(100))
    );
    assert_eq!(a["paidAt"], paid_at);
    // Cancelling gives 應退 (negative).
    assert_eq!(order(&f, "Amy", &[]).await.0, StatusCode::OK);
    let ps = participants(&f).await;
    assert_eq!(row(&ps, "Amy")["balance"], -200);
    assert_eq!(row(&ps, "Amy")["items"], json!([]));

    // Unpay clears paid state, keeps pickup; unpicking clears it.
    let (_, v) =
        f.c.put(&path, json!({"pickedUp": true, "paid": false}))
            .await;
    assert_eq!(
        (
            v["paid"].clone(),
            v["paidAt"].clone(),
            v["paidAmount"].clone(),
            v["balance"].clone(),
            v["pickedUp"].clone()
        ),
        (
            json!(false),
            Value::Null,
            Value::Null,
            Value::Null,
            json!(true)
        )
    );
    let (_, v) =
        f.c.put(&path, json!({"pickedUp": false, "paid": false}))
            .await;
    assert_eq!(
        (v["pickedUp"].clone(), v["pickedUpAt"].clone()),
        (json!(false), Value::Null)
    );
    // Re-paying baselines on the then-current due (0 after the cancel).
    let (_, v) =
        f.c.put(&path, json!({"pickedUp": false, "paid": true}))
            .await;
    assert_eq!(
        (v["paidAmount"].clone(), v["balance"].clone()),
        (json!(0), json!(0))
    );
}

#[tokio::test]
async fn participants_list_history_and_device_flags() {
    let f = Fixture::new().await;
    let id = gid(&f);
    let ua = "Mozilla/5.0 (iPhone; CPU iPhone OS 18_1 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Mobile/15E148 Safari Line/14.17.0";
    let long_device = "a3f9c2d1-1111-4222-8333-444455556666";
    // Amy submits twice from one device; Bob submits once from the same device.
    // Cat uses another device.
    for (who, qty, dev) in [
        ("Amy", 1, long_device),
        ("Amy", 2, long_device),
        ("Bob", 1, long_device),
        ("Cat", 1, "zzz-other-device"),
    ] {
        let (s, v, _) = send(
            &f.c.app,
            "POST",
            &format!("{}/participants/{}/submissions", f.base(), f.pid(who)),
            Some(json!({"lines": lines(&f, &[("排骨飯", qty)]), "idempotencyKey": key(), "note": "備註",
                        "client": {"deviceId": dev, "deviceModel": "Pixel 8", "screen": "390x844", "language": "zh-TW", "timezone": "Asia/Taipei"}})),
            Some("203.0.113.8"),
            &[("user-agent", ua)],
        )
        .await;
        assert_eq!(s, StatusCode::OK, "{v}");
    }
    let ps = participants(&f).await;
    let a = row(&ps, "Amy");
    assert_eq!(a["submissionCount"], 2);
    assert_eq!(a["suspiciousDevice"], true);
    assert_eq!(a["deviceShort"], "a3f");
    assert_eq!(
        a["items"],
        json!([{"itemId": f.item("排骨飯"), "name": "排骨飯", "quantity": 2, "note": null}])
    );
    assert_eq!(a["orderNote"], "備註");
    assert_eq!(a["amountDue"], 200);
    assert!(a["submittedAt"].is_string());
    assert_eq!(a["sortOrder"], 0);
    assert_eq!(row(&ps, "Bob")["suspiciousDevice"], true);
    assert_eq!(row(&ps, "Bob")["submissionCount"], 1);
    let c = row(&ps, "Cat");
    assert_eq!(
        (c["suspiciousDevice"].clone(), c["deviceShort"].clone()),
        (json!(false), json!("zzz"))
    );
    // Order of the list = sortOrder.
    let names: Vec<_> = ps
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["Amy", "Bob", "Cat"]);

    // A never-ordered participant.
    let (_, _) =
        f.c.post(
            &format!("/group-orders/{id}/participants"),
            json!({"name": "Dan"}),
        )
        .await;
    let ps = participants(&f).await;
    let d = row(&ps, "Dan");
    assert_eq!(
        (
            d["submissionCount"].clone(),
            d["submittedAt"].clone(),
            d["deviceShort"].clone(),
            d["suspiciousDevice"].clone(),
            d["amountDue"].clone()
        ),
        (json!(0), Value::Null, Value::Null, json!(false), json!(0))
    );

    // History: newest first, effective flag, shared device names, no raw UA / device id.
    let (s, h) =
        f.c.get(&format!(
            "/group-orders/{id}/participants/{}/submissions",
            f.pid("Amy")
        ))
        .await;
    assert_eq!(s, StatusCode::OK, "{h}");
    let h = h.as_array().unwrap();
    assert_eq!(h.len(), 2);
    assert_eq!(
        (h[0]["effective"].clone(), h[1]["effective"].clone()),
        (json!(true), json!(false))
    );
    assert_eq!(h[0]["total"], 200);
    assert_eq!(h[1]["total"], 100);
    assert_eq!(h[0]["submittedBy"], "participant");
    assert_eq!(h[0]["cancelled"], false);
    assert_eq!(h[0]["note"], "備註");
    assert_eq!(
        h[0]["lines"],
        json!([{"itemId": f.item("排骨飯"), "name": "排骨飯", "quantity": 2, "unitPrice": 100, "note": null}])
    );
    assert_eq!(h[0]["sharedDeviceWith"], json!(["Bob"]));
    let client = &h[0]["client"];
    assert_eq!(client["ip"], "203.0.113.8");
    assert_eq!(client["deviceShort"], "a3f");
    assert_eq!(client["deviceModel"], "Pixel 8");
    assert_eq!(client["screen"], "390x844");
    assert_eq!(client["language"], "zh-TW");
    assert_eq!(client["timezone"], "Asia/Taipei");
    assert!(client["uaSummary"].as_str().unwrap().contains("iPhone"));
    let raw = serde_json::to_string(&h).unwrap();
    assert!(
        !raw.contains("Mozilla") && !raw.contains("userAgent") && !raw.contains(long_device),
        "{raw}"
    );
    assert!(!serde_json::to_string(&ps).unwrap().contains(long_device));
    // Cat's history has no shared device.
    let (_, ch) =
        f.c.get(&format!(
            "/group-orders/{id}/participants/{}/submissions",
            f.pid("Cat")
        ))
        .await;
    assert_eq!(ch[0]["sharedDeviceWith"], json!([]));

    // Organizer correction: appears first with submittedBy organizer and no client info;
    // cancellation counts as a submission.
    let path = format!(
        "/group-orders/{id}/participants/{}/submissions",
        f.pid("Amy")
    );
    let k = key();
    let body = json!({"lines": [], "idempotencyKey": k});
    let (s, v) = f.c.post(&path, body.clone()).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["order"]["cancelled"], true);
    // Replaying the same idempotency key inserts nothing.
    let (s, _) = f.c.post(&path, body).await;
    assert_eq!(s, StatusCode::OK);
    let (_, h) = f.c.get(&path).await;
    assert_eq!(h.as_array().unwrap().len(), 3);
    assert_eq!(h[0]["submittedBy"], "organizer");
    assert_eq!(h[0]["cancelled"], true);
    assert_eq!(h[0]["total"], 0);
    assert_eq!(
        h[0]["client"],
        json!({"ip": null, "uaSummary": null, "deviceShort": null, "deviceModel": null, "screen": null, "language": null, "timezone": null})
    );
    assert_eq!(h[0]["sharedDeviceWith"], json!([]));
    let ps = participants(&f).await;
    assert_eq!(row(&ps, "Amy")["submissionCount"], 3);
    assert_eq!(row(&ps, "Amy")["deviceShort"], Value::Null);
    // Organizer submit validates like the public one.
    let (s, v) =
        f.c.post(
            &path,
            json!({"lines": [{"itemId": f.item("素食"), "quantity": 21}], "idempotencyKey": key()}),
        )
        .await;
    assert_eq!(
        (s, v["code"].as_str()),
        (StatusCode::UNPROCESSABLE_ENTITY, Some("invalid_quantity"))
    );
    let (s, v) =
        f.c.post(&path, json!({"lines": [], "idempotencyKey": "nope"}))
            .await;
    assert_eq!(
        (s, v["code"].as_str()),
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            Some("invalid_idempotencyKey")
        )
    );
}

#[tokio::test]
async fn openapi_documents_the_management_endpoints() {
    let doc = serde_json::to_value(crate::openapi()).unwrap();
    let p = &doc["paths"];
    for (path, method) in [
        ("/api/v1/group-orders", "get"),
        ("/api/v1/group-orders/{id}", "patch"),
        ("/api/v1/group-orders/{id}/share-token/rotate", "post"),
        ("/api/v1/group-orders/{id}/participants", "post"),
        ("/api/v1/group-orders/{id}/participants", "get"),
        ("/api/v1/group-orders/{id}/offered-items/{itemId}", "patch"),
        ("/api/v1/group-orders/{id}/summary", "get"),
        (
            "/api/v1/group-orders/{id}/participants/{pid}/submissions",
            "get",
        ),
        (
            "/api/v1/group-orders/{id}/participants/{pid}/submissions",
            "post",
        ),
        (
            "/api/v1/group-orders/{id}/participants/{pid}/settlement",
            "put",
        ),
    ] {
        assert_eq!(
            p[path][method]["tags"][0], "group-orders",
            "{method} {path}"
        );
    }
}
