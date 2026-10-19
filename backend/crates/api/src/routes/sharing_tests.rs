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
