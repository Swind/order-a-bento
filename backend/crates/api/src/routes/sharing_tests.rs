use super::directory_tests::{Client as BaseClient, login};
use axum::http::StatusCode;
use serde_json::{Value, json};

struct Client {
    base: BaseClient,
}
impl std::ops::Deref for Client {
    type Target = BaseClient;
    fn deref(&self) -> &BaseClient {
        &self.base
    }
}
impl Client {
    async fn new() -> Self {
        Self {
            base: BaseClient::new().await,
        }
    }
    async fn other(&self) -> Self {
        Self {
            base: self.base.other().await,
        }
    }
    async fn send(&self, method: &str, path: &str, body: Option<Value>) -> (StatusCode, Value) {
        self.base
            .send(method, path.strip_prefix("/api/v1").unwrap_or(path), body)
            .await
    }
}

async fn user(c: &Client) -> String {
    c.send("GET", "/api/v1/me", None).await.1["id"]
        .as_str()
        .unwrap()
        .into()
}
async fn group(c: &Client, name: &str) -> String {
    let (s, g) = c
        .send("POST", "/api/v1/sharing-groups", Some(json!({"name":name})))
        .await;
    assert_eq!(s, StatusCode::CREATED, "{g}");
    assert_eq!(g["role"], "admin");
    g["id"].as_str().unwrap().into()
}
async fn add(c: &Client, g: &str, id: &str, role: &str) {
    assert_eq!(
        c.send(
            "POST",
            &format!("/api/v1/sharing-groups/{g}/members"),
            Some(json!({"userId":id,"role":role}))
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
}

#[tokio::test]
async fn sharing_management_and_exact_identity_lookup() {
    let admin = Client::new().await;
    let member = admin.other().await;
    let uid = user(&member).await;
    let gid = group(&admin, "Team").await;
    let path = format!("/api/v1/sharing-groups/{gid}");
    assert_eq!(
        member.send("GET", &path, None).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        member.send("GET", "/api/v1/sharing-groups", None).await.1,
        json!([])
    );
    let lookup = format!("{path}/users/lookup?email=b%40x.test");
    assert_eq!(
        member.send("GET", &lookup, None).await.0,
        StatusCode::NOT_FOUND
    );
    let (s, found) = admin.send("GET", &lookup, None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(found[0]["id"], uid);
    assert!(found[0].get("passwordHash").is_none());
    assert!(found[0].get("oidcSub").is_none());
    assert_eq!(
        admin
            .send("GET", &format!("{path}/users/lookup?email=x.test"), None)
            .await
            .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        admin
            .send(
                "GET",
                &format!("{path}/users/lookup?email=%25%40x.test"),
                None
            )
            .await
            .1,
        json!([])
    );
    assert_eq!(
        admin.send("GET", &format!("{path}/users"), None).await.0,
        StatusCode::NOT_FOUND
    );
    add(&admin, &gid, &uid, "member").await;
    let (s, detail) = member.send("GET", &path, None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(detail["members"].as_array().unwrap().len(), 2);
    assert_eq!(detail["role"], "member");
    assert_eq!(
        member.send("GET", &lookup, None).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        member
            .send("PATCH", &path, Some(json!({"name":"no"})))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        member
            .send(
                "POST",
                &format!("{path}/members"),
                Some(json!({"userId":uid,"role":"admin"}))
            )
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        admin
            .send(
                "POST",
                &format!("{path}/members"),
                Some(json!({"userId":uid,"role":"member"}))
            )
            .await
            .1["code"],
        "already_member"
    );
    assert_eq!(
        admin
            .send(
                "POST",
                &format!("{path}/members"),
                Some(json!({"userId":uid,"role":"owner"}))
            )
            .await
            .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let own = user(&admin).await;
    for method in ["PATCH", "DELETE"] {
        let (s, e) = admin
            .send(
                method,
                &format!("{path}/members/{own}"),
                Some(json!({"role":"member"})),
            )
            .await;
        assert_eq!(s, StatusCode::CONFLICT);
        assert_eq!(e["code"], "last_admin");
    }
    assert_eq!(
        admin
            .send(
                "PATCH",
                &format!("{path}/members/{uid}"),
                Some(json!({"role":"admin"}))
            )
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        member
            .send("PATCH", &path, Some(json!({"name":"Renamed"})))
            .await
            .1["name"],
        "Renamed"
    );
    assert_eq!(
        member
            .send("DELETE", &format!("{path}/members/{own}"), None)
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        admin.send("GET", &path, None).await.0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn shared_directory_child_permissions_scope_isolation_and_revocation() {
    let admin = Client::new().await;
    let member = admin.other().await;
    let uid = user(&member).await;
    let gid = group(&admin, "Team").await;
    add(&admin, &gid, &uid, "member").await;
    let other_gid = group(&admin, "Other").await;
    let outsider = Client {
        base: BaseClient {
            app: admin.app.clone(),
            cookie: login(&admin.app, "outsider@x.test").await,
            _t: None,
        },
    };
    let mut resources = Vec::new();
    for (kind, body) in [
        (
            "rosters",
            json!({"name":"Shared","members":[{"name":"Alice"}]}),
        ),
        (
            "vendors",
            json!({"name":"Shared","menuItems":[{"name":"Rice","price":80}]}),
        ),
    ] {
        // Identical names in private and different shared scopes do not collide.
        assert_eq!(
            admin
                .send("POST", &format!("/api/v1/{kind}"), Some(body.clone()))
                .await
                .0,
            StatusCode::CREATED
        );
        let mut b = body.clone();
        b["sharingGroupId"] = json!(gid);
        let (s, r) = admin
            .send("POST", &format!("/api/v1/{kind}"), Some(b.clone()))
            .await;
        assert_eq!(s, StatusCode::CREATED, "{r}");
        assert_eq!(r["sharingGroupId"], gid);
        assert_eq!(r["canManage"], true);
        let id = r["id"].as_str().unwrap();
        let path = format!("/api/v1/{kind}/{id}");
        assert_eq!(member.send("GET", &path, None).await.1["canManage"], false);
        assert_eq!(
            outsider.send("GET", &path, None).await.0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            member
                .send("POST", &format!("/api/v1/{kind}"), Some(b.clone()))
                .await
                .0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            outsider
                .send("POST", &format!("/api/v1/{kind}"), Some(b.clone()))
                .await
                .0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            member.send("GET", &format!("/api/v1/{kind}"), None).await.1,
            json!([])
        );
        let list = member
            .send("GET", &format!("/api/v1/{kind}?sharingGroupId={gid}"), None)
            .await
            .1;
        assert_eq!(list.as_array().unwrap().len(), 1);
        assert_eq!(list[0]["canManage"], false);
        assert_eq!(
            member
                .send(
                    "GET",
                    &format!("/api/v1/{kind}?sharingGroupId={other_gid}"),
                    None
                )
                .await
                .0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            member
                .send("PATCH", &path, Some(json!({"name":"no"})))
                .await
                .0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            member.send("DELETE", &path, None).await.0,
            StatusCode::NOT_FOUND
        );
        let (child, child_id, upsert) = if kind == "rosters" {
            (
                "members",
                r["members"][0]["id"].as_str().unwrap(),
                json!({"members":[{"name":"Bob"}]}),
            )
        } else {
            (
                "menu-items",
                r["menuItems"][0]["id"].as_str().unwrap(),
                json!({"items":[{"name":"Soup","price":30}]}),
            )
        };
        let child_path = format!("{path}/{child}/{child_id}");
        let upsert_path = format!("{path}/{child}/upsert");
        for c in [&member, &outsider] {
            assert_eq!(
                c.send("PATCH", &child_path, Some(json!({"name":"no"})))
                    .await
                    .0,
                StatusCode::NOT_FOUND
            );
            assert_eq!(
                c.send("DELETE", &child_path, None).await.0,
                StatusCode::NOT_FOUND
            );
            assert_eq!(
                c.send("POST", &upsert_path, Some(upsert.clone())).await.0,
                StatusCode::NOT_FOUND
            );
        }
        // A second administrator, not the creator, can edit all child endpoints.
        assert_eq!(
            admin
                .send(
                    "PATCH",
                    &format!("/api/v1/sharing-groups/{gid}/members/{uid}"),
                    Some(json!({"role":"admin"}))
                )
                .await
                .0,
            StatusCode::NO_CONTENT
        );
        assert_eq!(
            member.send("POST", &upsert_path, Some(upsert)).await.0,
            StatusCode::OK
        );
        assert_eq!(
            member
                .send("PATCH", &child_path, Some(json!({"name":"Edited"})))
                .await
                .0,
            StatusCode::OK
        );
        assert_eq!(
            member.send("DELETE", &child_path, None).await.0,
            StatusCode::NO_CONTENT
        );
        assert_eq!(
            admin
                .send(
                    "PATCH",
                    &format!("/api/v1/sharing-groups/{gid}/members/{uid}"),
                    Some(json!({"role":"member"}))
                )
                .await
                .0,
            StatusCode::NO_CONTENT
        );
        b["sharingGroupId"] = json!(other_gid);
        assert_eq!(
            admin
                .send("POST", &format!("/api/v1/{kind}"), Some(b))
                .await
                .0,
            StatusCode::CREATED
        );
        // Supplying a scope in a patch cannot transfer a shared resource.
        let (_, unchanged) = admin
            .send(
                "PATCH",
                &path,
                Some(json!({"sharingGroupId":null,"name":"Still shared"})),
            )
            .await;
        assert_eq!(unchanged["sharingGroupId"], gid);
        resources.push(path);
    }
    assert_eq!(
        admin
            .send(
                "DELETE",
                &format!("/api/v1/sharing-groups/{gid}/members/{uid}"),
                None
            )
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    for p in resources {
        assert_eq!(member.send("GET", &p, None).await.0, StatusCode::NOT_FOUND);
    }
    // Even the resource creator loses access when removed from the group.
    add(&admin, &gid, &uid, "admin").await;
    let own = user(&admin).await;
    assert_eq!(
        member
            .send(
                "DELETE",
                &format!("/api/v1/sharing-groups/{gid}/members/{own}"),
                None
            )
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        admin
            .send(
                "GET",
                &format!("/api/v1/vendors?sharingGroupId={gid}"),
                None
            )
            .await
            .0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn member_opens_personal_order_and_snapshot_survives_revocation() {
    let admin = Client::new().await;
    let member = admin.other().await;
    let uid = user(&member).await;
    let gid = group(&admin, "Team").await;
    add(&admin, &gid, &uid, "member").await;
    let (_, r) = admin
        .send(
            "POST",
            "/api/v1/rosters",
            Some(json!({"name":"R","sharingGroupId":gid,"members":[{"name":"Alice"}]})),
        )
        .await;
    let (_, v) = admin
        .send(
            "POST",
            "/api/v1/vendors",
            Some(json!({"name":"V","sharingGroupId":gid,"menuItems":[{"name":"Rice","price":80}]})),
        )
        .await;
    let cmd = json!({"title":"Lunch","rosterId":r["id"],"vendorId":v["id"]});
    let (s, order) = member
        .send("POST", "/api/v1/group-orders", Some(cmd.clone()))
        .await;
    assert_eq!(s, StatusCode::CREATED, "{order}");
    let path = format!("/api/v1/group-orders/{}", order["id"].as_str().unwrap());
    assert_eq!(
        admin.send("GET", &path, None).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        admin
            .send(
                "DELETE",
                &format!("/api/v1/sharing-groups/{gid}/members/{uid}"),
                None
            )
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        member
            .send("POST", "/api/v1/group-orders", Some(cmd))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        admin
            .send(
                "DELETE",
                &format!("/api/v1/vendors/{}", v["id"].as_str().unwrap()),
                None
            )
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    let (s, after) = member.send("GET", &path, None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(after["vendor"], order["vendor"]);
    assert_eq!(after["participants"], order["participants"]);
    assert_eq!(after["offeredItems"], order["offeredItems"]);
}

#[test]
fn sharing_openapi_contract() {
    let spec = serde_json::to_value(crate::openapi()).unwrap();
    for path in [
        "/api/v1/sharing-groups",
        "/api/v1/sharing-groups/{groupId}",
        "/api/v1/sharing-groups/{groupId}/members",
        "/api/v1/sharing-groups/{groupId}/members/{userId}",
        "/api/v1/sharing-groups/{groupId}/users/lookup",
    ] {
        assert!(spec["paths"][path].is_object(), "{path}");
    }
    for name in [
        "GroupOrderDetail",
        "GroupListItemDto",
        "RosterDetail",
        "RosterSummaryDto",
        "VendorDetail",
        "VendorSummaryDto",
    ] {
        assert!(spec["components"]["schemas"][name]["properties"]["canManage"].is_object());
        assert!(spec["components"]["schemas"][name]["properties"]["sharingGroupId"].is_object());
    }
    let params = &spec["paths"]["/api/v1/rosters"]["get"]["parameters"];
    assert!(
        params
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["name"] == "sharingGroupId")
    );
    let _: Value = spec;
}

fn order_mutations(order: &Value) -> Vec<(&'static str, String, Value)> {
    let id = order["id"].as_str().unwrap();
    let pid = order["participants"][0]["id"].as_str().unwrap();
    let item = order["offeredItems"][0]["id"].as_str().unwrap();
    let base = format!("/api/v1/group-orders/{id}");
    vec![
        ("PATCH", base.clone(), json!({"title":"Updated"})),
        ("POST", format!("{base}/share-token/rotate"), json!({})),
        (
            "POST",
            format!("{base}/participants"),
            json!({"name":uuid::Uuid::now_v7().to_string()}),
        ),
        (
            "PATCH",
            format!("{base}/offered-items/{item}"),
            json!({"soldOut":false}),
        ),
        (
            "POST",
            format!("{base}/participants/{pid}/submissions"),
            json!({"lines":[],"idempotencyKey":uuid::Uuid::now_v7().to_string()}),
        ),
        (
            "PUT",
            format!("{base}/participants/{pid}/settlement"),
            json!({"pickedUp":true,"paid":true}),
        ),
    ]
}

async fn assert_order_access(
    c: &Client,
    order: &Value,
    read: StatusCode,
    write: Option<StatusCode>,
) {
    let id = order["id"].as_str().unwrap();
    let pid = order["participants"][0]["id"].as_str().unwrap();
    for suffix in [
        String::new(),
        "/summary".into(),
        "/participants".into(),
        format!("/participants/{pid}/submissions"),
    ] {
        let path = format!("/api/v1/group-orders/{id}{suffix}");
        let (s, v) = c.send("GET", &path, None).await;
        assert_eq!(s, read, "{path}: {v}");
    }
    let list = c.send("GET", "/api/v1/group-orders", None).await.1;
    assert_eq!(
        list.as_array()
            .unwrap()
            .iter()
            .any(|g| g["id"] == order["id"]),
        read == StatusCode::OK
    );
    for (method, path, body) in order_mutations(order) {
        let (s, v) = c.send(method, &path, Some(body)).await;
        if let Some(expected) = write {
            assert_eq!(s, expected, "{method} {path}: {v}");
        } else {
            assert!(s.is_success(), "{method} {path}: {s} {v}");
        }
    }
}

#[tokio::test]
async fn group_order_endpoint_authorization_matrix_and_revocation() {
    let admin = Client::new().await;
    let creator = admin.other().await;
    let reader = Client {
        base: BaseClient {
            app: admin.app.clone(),
            cookie: login(&admin.app, "reader@x.test").await,
            _t: None,
        },
    };
    let outsider = Client {
        base: BaseClient {
            app: admin.app.clone(),
            cookie: login(&admin.app, "outsider@x.test").await,
            _t: None,
        },
    };
    let gid = group(&admin, "Team").await;
    let creator_id = user(&creator).await;
    let reader_id = user(&reader).await;
    add(&admin, &gid, &creator_id, "member").await;
    add(&admin, &gid, &reader_id, "member").await;
    // A member may use personal snapshots for a group-owned order.
    let (_, roster) = creator
        .post(
            "/rosters",
            json!({"name":"R","members":[{"name":"Alice"},{"name":"Bob"}]}),
        )
        .await;
    let (_, vendor) = creator
        .post(
            "/vendors",
            json!({"name":"V","menuItems":[{"name":"Rice","price":80}]}),
        )
        .await;
    let cmd = json!({"title":"Lunch","rosterId":roster["id"],"vendorId":vendor["id"],"sharingGroupId":gid});
    let (s, order) = creator
        .send("POST", "/api/v1/group-orders", Some(cmd.clone()))
        .await;
    assert_eq!(s, StatusCode::CREATED, "{order}");
    assert_eq!(order["canManage"], true);
    assert_eq!(order["sharingGroupId"], gid);
    assert_eq!(order["sharingGroupName"], "Team");
    let base = format!("/api/v1/group-orders/{}", order["id"].as_str().unwrap());
    assert_eq!(reader.send("GET", &base, None).await.1["canManage"], false);
    assert_order_access(&reader, &order, StatusCode::OK, Some(StatusCode::FORBIDDEN)).await;
    assert_order_access(
        &outsider,
        &order,
        StatusCode::NOT_FOUND,
        Some(StatusCode::NOT_FOUND),
    )
    .await;
    assert_order_access(&creator, &order, StatusCode::OK, None).await;
    // Admin is not the creator, but may manage every mutation endpoint.
    assert_order_access(&admin, &order, StatusCode::OK, None).await;
    let member_path = format!("/api/v1/sharing-groups/{gid}/members/{reader_id}");
    assert_eq!(
        admin
            .send("PATCH", &member_path, Some(json!({"role":"admin"})))
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(reader.send("GET", &base, None).await.1["canManage"], true);
    assert_order_access(&reader, &order, StatusCode::OK, None).await;
    assert_eq!(
        admin
            .send("PATCH", &member_path, Some(json!({"role":"member"})))
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    assert_order_access(&reader, &order, StatusCode::OK, Some(StatusCode::FORBIDDEN)).await;
    // Removal revokes creator privileges, including every child and mutation.
    assert_eq!(
        admin
            .send(
                "DELETE",
                &format!("/api/v1/sharing-groups/{gid}/members/{creator_id}"),
                None
            )
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    assert_order_access(
        &creator,
        &order,
        StatusCode::NOT_FOUND,
        Some(StatusCode::NOT_FOUND),
    )
    .await;
    assert_eq!(
        creator
            .send("POST", "/api/v1/group-orders", Some(cmd))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        admin.send("DELETE", &member_path, None).await.0,
        StatusCode::NO_CONTENT
    );
    assert_order_access(
        &reader,
        &order,
        StatusCode::NOT_FOUND,
        Some(StatusCode::NOT_FOUND),
    )
    .await;
}

#[tokio::test]
async fn shared_order_history_redacts_client_metadata_and_archived_orders_are_readonly() {
    let admin = Client::new().await;
    let reader = admin.other().await;
    let gid = group(&admin, "Privacy").await;
    add(&admin, &gid, &user(&reader).await, "member").await;
    let (_, roster) = admin
        .post(
            "/rosters",
            json!({"name":"R","members":[{"name":"Alice"},{"name":"Bob"}]}),
        )
        .await;
    let (_, vendor) = admin
        .post(
            "/vendors",
            json!({"name":"V","menuItems":[{"name":"Rice","price":80}]}),
        )
        .await;
    let (s, order) = admin.send("POST", "/api/v1/group-orders", Some(json!({"title":"Lunch","rosterId":roster["id"],"vendorId":vendor["id"],"sharingGroupId":gid}))).await;
    assert_eq!(s, StatusCode::CREATED, "{order}");
    let token = order["shareUrl"]
        .as_str()
        .unwrap()
        .rsplit('/')
        .next()
        .unwrap();
    for p in order["participants"].as_array().unwrap() {
        let path = format!(
            "/api/v1/public/g/{token}/participants/{}/submissions",
            p["id"].as_str().unwrap()
        );
        let body = json!({"lines":[{"itemId":order["offeredItems"][0]["id"],"quantity":1}],"idempotencyKey":uuid::Uuid::now_v7().to_string(),"client":{"deviceId":"abc123","deviceModel":"Pixel","screen":"100x100","language":"zh","timezone":"Asia/Taipei"}});
        let (s, v, _) = super::ordering_tests::send(
            &admin.app,
            "POST",
            &path,
            Some(body),
            Some("1.2.3.4"),
            &[(
                "user-agent",
                "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 Chrome/120.0.0.0 Safari/537.36",
            )],
        )
        .await;
        assert_eq!(s, StatusCode::OK, "{v}");
    }
    let base = format!("/api/v1/group-orders/{}", order["id"].as_str().unwrap());
    let history = format!(
        "{base}/participants/{}/submissions",
        order["participants"][0]["id"].as_str().unwrap()
    );
    let manager_history = admin.send("GET", &history, None).await.1;
    assert_eq!(manager_history[0]["client"]["ip"], "1.2.3.4");
    assert_eq!(manager_history[0]["client"]["deviceShort"], "abc");
    assert_eq!(manager_history[0]["sharedDeviceWith"], json!(["Bob"]));
    let member_history = reader.send("GET", &history, None).await.1;
    assert!(
        member_history[0]["client"]
            .as_object()
            .unwrap()
            .values()
            .all(Value::is_null)
    );
    assert_eq!(member_history[0]["sharedDeviceWith"], json!([]));
    assert_eq!(member_history[0]["lines"], manager_history[0]["lines"]);
    let rows = admin
        .send("GET", &format!("{base}/participants"), None)
        .await
        .1;
    assert_eq!(rows[0]["suspiciousDevice"], true);
    let rows = reader
        .send("GET", &format!("{base}/participants"), None)
        .await
        .1;
    assert_eq!(rows[0]["suspiciousDevice"], false);
    assert!(rows[0]["deviceShort"].is_null());
    // Public deadline enforcement stays separate from manager authorization.
    sqlx::query("UPDATE group_orders SET deadline_at='2000-01-01T00:00:00Z' WHERE id=?")
        .bind(order["id"].as_str().unwrap())
        .execute(&admin._t.as_ref().unwrap().state.db.write)
        .await
        .unwrap();
    let public_path = format!(
        "/api/v1/public/g/{token}/participants/{}/submissions",
        order["participants"][0]["id"].as_str().unwrap()
    );
    let body = json!({"lines":[],"idempotencyKey":uuid::Uuid::now_v7().to_string()});
    assert_eq!(
        super::ordering_tests::send(
            &admin.app,
            "POST",
            &public_path,
            Some(body.clone()),
            None,
            &[]
        )
        .await
        .1["code"],
        "deadline_passed"
    );
    assert_eq!(
        admin.send("POST", &history, Some(body)).await.0,
        StatusCode::OK
    );
    assert_eq!(
        admin
            .send("PATCH", &base, Some(json!({"status":"closed"})))
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        admin
            .send("PATCH", &base, Some(json!({"status":"archived"})))
            .await
            .0,
        StatusCode::OK
    );
    for (method, path, body) in order_mutations(&order) {
        let expected = if path.ends_with("/submissions") {
            StatusCode::CONFLICT
        } else {
            StatusCode::UNPROCESSABLE_ENTITY
        };
        let (s, v) = admin.send(method, &path, Some(body)).await;
        assert_eq!(s, expected, "{path}: {v}");
    }
    assert_order_access(&reader, &order, StatusCode::OK, Some(StatusCode::FORBIDDEN)).await;
    let after = reader.send("GET", &history, None).await.1;
    assert!(
        after[1]["client"]
            .as_object()
            .unwrap()
            .values()
            .all(Value::is_null)
    );
}

#[tokio::test]
async fn order_ownership_is_independent_of_directory_scope_and_personal_is_default() {
    let admin = Client::new().await;
    let creator = admin.other().await;
    let uid = user(&creator).await;
    let source = group(&admin, "Directory source").await;
    let target = group(&admin, "Order owner").await;
    add(&admin, &source, &uid, "member").await;
    add(&admin, &target, &uid, "member").await;
    let (_, roster) = admin
        .post(
            "/rosters",
            json!({"name":"Shared R","sharingGroupId":source,"members":[{"name":"Alice"}]}),
        )
        .await;
    let (_, vendor) = admin.post("/vendors", json!({"name":"Shared V","sharingGroupId":source,"menuItems":[{"name":"Rice","price":80}]})).await;
    let mut cmd = json!({"title":"Cross scope","rosterId":roster["id"],"vendorId":vendor["id"],"sharingGroupId":target});
    let (s, shared) = creator
        .send("POST", "/api/v1/group-orders", Some(cmd.clone()))
        .await;
    assert_eq!(s, StatusCode::CREATED, "{shared}");
    assert_eq!(shared["sharingGroupId"], target);
    assert_eq!(shared["sharingGroupName"], "Order owner");
    let mut personal = Vec::new();
    for omitted in [false, true] {
        cmd["sharingGroupId"] = Value::Null;
        if omitted {
            cmd.as_object_mut().unwrap().remove("sharingGroupId");
        }
        let (s, g) = creator
            .send("POST", "/api/v1/group-orders", Some(cmd.clone()))
            .await;
        assert_eq!(s, StatusCode::CREATED, "{g}");
        assert_eq!(g["canManage"], true);
        assert!(g["sharingGroupId"].is_null());
        assert!(g["sharingGroupName"].is_null());
        assert_order_access(
            &admin,
            &g,
            StatusCode::NOT_FOUND,
            Some(StatusCode::NOT_FOUND),
        )
        .await;
        personal.push(g);
    }
    // Names are live group metadata, not copied from directory scope.
    assert_eq!(
        admin
            .send(
                "PATCH",
                &format!("/api/v1/sharing-groups/{target}"),
                Some(json!({"name":"Renamed"}))
            )
            .await
            .0,
        StatusCode::OK
    );
    let path = format!("/api/v1/group-orders/{}", shared["id"].as_str().unwrap());
    assert_eq!(
        creator.send("GET", &path, None).await.1["sharingGroupName"],
        "Renamed"
    );
    let list = creator
        .send("GET", "/api/v1/group-orders?status=open", None)
        .await
        .1;
    assert_eq!(list.as_array().unwrap().len(), 3);
    assert!(
        list.as_array()
            .unwrap()
            .iter()
            .any(|g| g["id"] == shared["id"]
                && g["sharingGroupName"] == "Renamed"
                && g["canManage"] == true)
    );
    // Revoking directory access does not revoke access to existing snapshots.
    assert_eq!(
        admin
            .send(
                "DELETE",
                &format!("/api/v1/sharing-groups/{source}/members/{uid}"),
                None
            )
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    assert_order_access(&creator, &shared, StatusCode::OK, None).await;
    assert_eq!(
        creator
            .send("POST", "/api/v1/group-orders", Some(cmd))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    // Revoking target membership does revoke the shared order, but not personal orders.
    assert_eq!(
        admin
            .send(
                "DELETE",
                &format!("/api/v1/sharing-groups/{target}/members/{uid}"),
                None
            )
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    assert_order_access(
        &creator,
        &shared,
        StatusCode::NOT_FOUND,
        Some(StatusCode::NOT_FOUND),
    )
    .await;
    for g in personal {
        assert_order_access(&creator, &g, StatusCode::OK, None).await;
    }
}

#[tokio::test]
async fn manager_idempotent_retry_survives_sold_out_but_not_membership_removal() {
    let admin = Client::new().await;
    let creator = admin.other().await;
    let uid = user(&creator).await;
    let gid = group(&admin, "Retries").await;
    add(&admin, &gid, &uid, "member").await;
    let (_, roster) = creator
        .post("/rosters", json!({"name":"R","members":[{"name":"Alice"}]}))
        .await;
    let (_, vendor) = creator
        .post(
            "/vendors",
            json!({"name":"V","menuItems":[{"name":"Rice","price":80}]}),
        )
        .await;
    let (_, order) = creator.send("POST", "/api/v1/group-orders", Some(json!({"title":"Lunch","rosterId":roster["id"],"vendorId":vendor["id"],"sharingGroupId":gid}))).await;
    let base = format!("/api/v1/group-orders/{}", order["id"].as_str().unwrap());
    let submissions = format!(
        "{base}/participants/{}/submissions",
        order["participants"][0]["id"].as_str().unwrap()
    );
    let item = order["offeredItems"][0]["id"].as_str().unwrap();
    let cmd = json!({"lines":[{"itemId":item,"quantity":1}],"idempotencyKey":uuid::Uuid::now_v7().to_string()});
    let (s, first) = creator.send("POST", &submissions, Some(cmd.clone())).await;
    assert_eq!(s, StatusCode::OK, "{first}");
    assert_eq!(
        admin
            .send(
                "PATCH",
                &format!("{base}/offered-items/{item}"),
                Some(json!({"soldOut":true}))
            )
            .await
            .0,
        StatusCode::OK
    );
    let (s, retry) = creator.send("POST", &submissions, Some(cmd.clone())).await;
    assert_eq!(s, StatusCode::OK, "{retry}");
    assert_eq!(first, retry);
    assert_eq!(
        admin
            .send(
                "DELETE",
                &format!("/api/v1/sharing-groups/{gid}/members/{uid}"),
                None
            )
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        creator.send("POST", &submissions, Some(cmd)).await.0,
        StatusCode::NOT_FOUND
    );
}
