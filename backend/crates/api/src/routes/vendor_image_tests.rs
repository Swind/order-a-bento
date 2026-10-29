use super::directory_tests::{Client, login};
use axum::{
    body::Body,
    http::{Request, StatusCode, header},
    response::Response,
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

fn png() -> Vec<u8> {
    use image::ImageEncoder;
    let mut bytes = Vec::new();
    image::codecs::png::PngEncoder::new(&mut bytes)
        .write_image(
            &[255, 0, 0, 255, 0, 255, 0, 255],
            2,
            1,
            image::ExtendedColorType::Rgba8,
        )
        .unwrap();
    bytes
}

fn multipart(parts: &[(&str, &[u8])]) -> Vec<u8> {
    let mut body = Vec::new();
    for (name, bytes) in parts {
        body.extend_from_slice(format!("--boundary\r\nContent-Disposition: form-data; name=\"{name}\"; filename=\"untrusted.html\"\r\nContent-Type: text/html\r\n\r\n").as_bytes());
        body.extend_from_slice(bytes);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(b"--boundary--\r\n");
    body
}

async fn request(c: &Client, method: &str, path: &str, body: Vec<u8>) -> Response {
    c.app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(format!("/api/v1{path}"))
                .header(header::COOKIE, &c.cookie)
                .header(
                    header::CONTENT_TYPE,
                    "multipart/form-data; boundary=boundary",
                )
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap()
}

async fn value(response: Response) -> Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

async fn client() -> Client {
    let mut c = Client::new().await;
    let t = c._t.as_mut().unwrap();
    t.state.image_limiter = std::sync::Arc::new(crate::ratelimit::KeyedLimiter::new(
        crate::RateLimitConfig {
            burst: 1000,
            per_sec: 1000.0,
        },
    ));
    c.app = crate::router(t.state.clone());
    c
}

#[tokio::test]
async fn private_image_round_trip_atomic_replace_menu_metadata_and_restart() {
    let c = client().await;
    let (s, vendor) = c
        .post(
            "/vendors",
            json!({"name":"Photos", "menuItems":[{"name":"Rice","price":100}]}),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED);
    assert!(vendor["image"].is_null());
    let id = vendor["id"].as_str().unwrap();
    let parent = format!("/vendors/{id}");
    let path = format!("{parent}/image");
    let response = request(&c, "PUT", &path, multipart(&[("image", &png())])).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()[header::CACHE_CONTROL],
        "private, no-store"
    );
    let first = value(response).await;
    assert_eq!(first["contentType"], "image/jpeg");
    assert_eq!(first["width"], 2);
    assert_eq!(first["height"], 1);
    let response = request(&c, "GET", &path, vec![]).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "image/jpeg");
    assert_eq!(
        response.headers()[header::CACHE_CONTROL],
        "private, no-store"
    );
    assert_eq!(
        response.headers()[header::X_CONTENT_TYPE_OPTIONS],
        "nosniff"
    );
    assert_eq!(
        response.headers()["cross-origin-resource-policy"],
        "same-origin"
    );
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(bytes.len(), first["sizeBytes"].as_u64().unwrap() as usize);
    assert_eq!(image::load_from_memory(&bytes).unwrap().width(), 2);
    let invalid = request(&c, "PUT", &path, multipart(&[("image", b"<html/>")])).await;
    assert_eq!(invalid.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(value(invalid).await["field"], "image");
    assert_eq!(c.get(&parent).await.1["image"], first);
    let (s, changed) = c.patch(&parent, json!({"note":"keep me"})).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(changed["image"], first);
    let (s, menu) = c
        .post(
            &format!("{parent}/menu-items/upsert"),
            json!({"items":[{"name":"Soup","price":20}]}),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(menu["vendor"]["image"], first);
    let replacement = request(&c, "PUT", &path, multipart(&[("image", &png())])).await;
    let second = value(replacement).await;
    assert_ne!(first["revision"], second["revision"]);
    assert_eq!(c.get(&parent).await.1["note"], "keep me");
    assert_eq!(
        c.get(&parent).await.1["menuItems"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    // A separate pool reading the same file proves this is not process-only storage.
    let t = c._t.as_ref().unwrap();
    let reopened = infrastructure::sqlite::Db::connect_path(&t._dir.path().join("test.db"))
        .await
        .unwrap();
    let (stored, count): (Vec<u8>, i64) = sqlx::query_as(
        "SELECT data,(SELECT count(*) FROM vendor_images) FROM vendor_images WHERE vendor_id=?",
    )
    .bind(id)
    .fetch_one(&reopened.read)
    .await
    .unwrap();
    assert_eq!(stored, bytes);
    assert_eq!(count, 1);
    for _ in 0..2 {
        assert_eq!(
            request(&c, "DELETE", &path, vec![]).await.status(),
            StatusCode::NO_CONTENT
        );
    }
    assert!(c.get(&parent).await.1["image"].is_null());
    assert_eq!(
        request(&c, "GET", &path, vec![]).await.status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(&c, "PUT", &path, multipart(&[("image", &png())]))
            .await
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        c.send("DELETE", &parent, None).await.0,
        StatusCode::NO_CONTENT
    );
    let (count,): (i64,) = sqlx::query_as("SELECT count(*) FROM vendor_images")
        .fetch_one(&t.state.db.read)
        .await
        .unwrap();
    assert_eq!(count, 0);
    assert_eq!(
        request(&c, "PUT", &path, multipart(&[("image", &png())]))
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn rejects_multipart_and_streaming_size_limits_without_replacing() {
    let c = client().await;
    let (_, vendor) = c.post("/vendors", json!({"name":"Validation"})).await;
    let parent = format!("/vendors/{}", vendor["id"].as_str().unwrap());
    let path = format!("{parent}/image");
    let first = value(request(&c, "PUT", &path, multipart(&[("image", &png())])).await).await;
    let cases = [
        (
            multipart(&[]),
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_image",
        ),
        (
            multipart(&[("image", b"")]),
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_image",
        ),
        (
            multipart(&[("other", &png())]),
            StatusCode::BAD_REQUEST,
            "invalid_multipart",
        ),
        (
            multipart(&[("image", &png()), ("image", &png())]),
            StatusCode::BAD_REQUEST,
            "invalid_multipart",
        ),
        (
            b"garbage".to_vec(),
            StatusCode::BAD_REQUEST,
            "invalid_multipart",
        ),
        (
            multipart(&[("image", b"\xff\xd8\xff")]),
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_image",
        ),
        (
            multipart(&[("image", &vec![0; 5 * 1024 * 1024 + 1])]),
            StatusCode::PAYLOAD_TOO_LARGE,
            "image_too_large",
        ),
        (
            multipart(&[("image", &vec![0; 6 * 1024 * 1024 + 1])]),
            StatusCode::PAYLOAD_TOO_LARGE,
            "image_too_large",
        ),
    ];
    for (body, status, code) in cases {
        let response = request(&c, "PUT", &path, body).await;
        assert_eq!(response.status(), status);
        assert_eq!(
            response.headers()[header::CONTENT_TYPE],
            "application/problem+json"
        );
        assert_eq!(
            response.headers()[header::CACHE_CONTROL],
            "private, no-store"
        );
        assert_eq!(value(response).await["code"], code);
        assert_eq!(c.get(&parent).await.1["image"], first);
    }
    // Body without a size hint/Content-Length (same path used by chunked requests).
    let body = multipart(&[("image", &vec![0; 6 * 1024 * 1024 + 1])]);
    let frames = body
        .chunks(4096)
        .map(|c| Ok::<_, std::io::Error>(axum::body::Bytes::copy_from_slice(c)))
        .collect::<Vec<_>>();
    let stream = futures_util::stream::iter(frames);
    let req = Request::put(format!("/api/v1{path}"))
        .header(header::COOKIE, &c.cookie)
        .header(
            header::CONTENT_TYPE,
            "multipart/form-data; boundary=boundary",
        )
        .body(Body::from_stream(stream))
        .unwrap();
    let response = c.app.clone().oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(value(response).await["code"], "image_too_large");

    // The image boundary is inclusive: exactly 5 MiB reaches decoding, not 413.
    let response = request(
        &c,
        "PUT",
        &path,
        multipart(&[("image", &vec![0; 5 * 1024 * 1024])]),
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    // Exercise the body cap independently of the image cap using a multipart
    // preamble, rather than an image that already exceeds its own 5 MiB limit.
    let image_part = multipart(&[("image", &png())]);
    let mut exact = vec![b'x'; 6 * 1024 * 1024 - image_part.len() - 2];
    exact.extend_from_slice(b"\r\n");
    exact.extend_from_slice(&image_part);
    assert_eq!(
        request(&c, "PUT", &path, exact.clone()).await.status(),
        StatusCode::OK
    );
    exact.insert(0, b'x');
    let response = request(&c, "PUT", &path, exact).await;
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(value(response).await["code"], "image_too_large");
}

#[tokio::test]
async fn session_origin_permissions_and_processing_admission() {
    let c = client().await;
    let other = c.other().await;
    let (_, vendor) = c.post("/vendors", json!({"name":"Permissions"})).await;
    let path = format!("/vendors/{}/image", vendor["id"].as_str().unwrap());
    for method in ["GET", "PUT", "DELETE"] {
        let response = c
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(format!("/api/v1{path}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            response.headers()[header::CACHE_CONTROL],
            "private, no-store"
        );
        assert_eq!(
            request(&other, method, &path, vec![]).await.status(),
            StatusCode::NOT_FOUND
        );
    }
    let response = c
        .app
        .clone()
        .oneshot(
            Request::put(format!("/api/v1{path}"))
                .header(header::COOKIE, &c.cookie)
                .header(header::ORIGIN, "https://evil.example")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        response.headers()[header::CACHE_CONTROL],
        "private, no-store"
    );
    assert_eq!(value(response).await["code"], "bad_origin");
    let state = &c._t.as_ref().unwrap().state;
    let one = state
        .image_processing
        .clone()
        .acquire_owned()
        .await
        .unwrap();
    let two = state
        .image_processing
        .clone()
        .acquire_owned()
        .await
        .unwrap();
    let response = request(&c, "PUT", &path, vec![]).await;
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(response.headers().contains_key(header::RETRY_AFTER));
    drop((one, two));
    assert_eq!(
        request(&c, "PUT", &path, multipart(&[("image", &png())]))
            .await
            .status(),
        StatusCode::OK
    );

    let cookie = login(&c.app, "third@x.test").await;
    let third = Client {
        app: c.app.clone(),
        cookie,
        _t: None,
    };
    assert_eq!(
        request(&third, "GET", &path, vec![]).await.status(),
        StatusCode::NOT_FOUND
    );
}

#[test]
fn openapi_documents_required_nullable_metadata_and_binary_contracts() {
    let spec = serde_json::to_value(crate::openapi()).unwrap();
    let schemas = &spec["components"]["schemas"];
    assert!(
        schemas["VendorDetail"]["required"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "image")
    );
    assert_eq!(
        schemas["VendorImageForm"]["properties"]["image"]["format"],
        "binary"
    );
    assert_eq!(schemas["ImageBinary"]["format"], "binary");
    assert!(
        spec["paths"]["/api/v1/vendors/{vendorId}/image"]["get"]["responses"]["200"]["content"]
            .get("image/jpeg")
            .is_some()
    );
}

#[tokio::test]
async fn group_roles_removed_creator_and_revocation_during_upload() {
    let admin = client().await;
    let member = admin.other().await;
    let aid = admin.get("/me").await.1["id"].as_str().unwrap().to_owned();
    let mid = member.get("/me").await.1["id"].as_str().unwrap().to_owned();
    let (_, group) = admin
        .post("/sharing-groups", json!({"name":"Image team"}))
        .await;
    let gid = group["id"].as_str().unwrap();
    let members = format!("/sharing-groups/{gid}/members");
    assert_eq!(
        admin
            .post(&members, json!({"userId":mid,"role":"admin"}))
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    let (_, vendor) = admin
        .post("/vendors", json!({"name":"Shared", "sharingGroupId":gid}))
        .await;
    let id = vendor["id"].as_str().unwrap();
    let path = format!("/vendors/{id}/image");
    assert_eq!(
        request(&member, "PUT", &path, multipart(&[("image", &png())]))
            .await
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        admin
            .patch(&format!("{members}/{mid}"), json!({"role":"member"}))
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        request(&member, "GET", &path, vec![]).await.status(),
        StatusCode::OK
    );
    for method in ["PUT", "DELETE"] {
        assert_eq!(
            request(&member, method, &path, vec![]).await.status(),
            StatusCode::NOT_FOUND
        );
    }
    assert_eq!(
        admin
            .patch(&format!("{members}/{mid}"), json!({"role":"admin"}))
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    // The body stream is first polled after the manager precheck. Suspend it
    // there, revoke membership, then allow decoding and transaction recheck.
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (resume_tx, resume_rx) = tokio::sync::oneshot::channel();
    let body = multipart(&[("image", &png())]);
    let stream = futures_util::stream::once(async move {
        started_tx.send(()).unwrap();
        resume_rx.await.unwrap();
        Ok::<_, std::io::Error>(body)
    });
    let req = Request::put(format!("/api/v1{path}"))
        .header(header::COOKIE, &member.cookie)
        .header(
            header::CONTENT_TYPE,
            "multipart/form-data; boundary=boundary",
        )
        .body(Body::from_stream(stream))
        .unwrap();
    let pending = tokio::spawn(member.app.clone().oneshot(req));
    started_rx.await.unwrap();
    let before = admin.get(&format!("/vendors/{id}")).await.1["image"].clone();
    assert_eq!(
        admin
            .send("DELETE", &format!("{members}/{mid}"), None)
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    resume_tx.send(()).unwrap();
    let response = pending.await.unwrap().unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        admin.get(&format!("/vendors/{id}")).await.1["image"],
        before
    );
    assert_eq!(
        request(&member, "GET", &path, vec![]).await.status(),
        StatusCode::NOT_FOUND
    );
    // A former creator has no personal-owner bypass for a shared resource.
    assert_eq!(
        admin
            .post(&members, json!({"userId":mid,"role":"admin"}))
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        member
            .send("DELETE", &format!("{members}/{aid}"), None)
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    for method in ["GET", "PUT", "DELETE"] {
        assert_eq!(
            request(&admin, method, &path, vec![]).await.status(),
            StatusCode::NOT_FOUND
        );
    }
    assert_eq!(
        request(&member, "GET", &path, vec![]).await.status(),
        StatusCode::OK
    );
}

#[tokio::test]
async fn image_rate_limiter_is_independent_and_sets_retry_after() {
    let mut c = Client::new().await;
    let t = c._t.as_mut().unwrap();
    t.state.image_limiter = std::sync::Arc::new(crate::ratelimit::KeyedLimiter::new(
        crate::RateLimitConfig {
            burst: 1,
            per_sec: 0.001,
        },
    ));
    c.app = crate::router(t.state.clone());
    let (_, vendor) = c.post("/vendors", json!({"name":"Rate"})).await;
    let path = format!("/vendors/{}/image", vendor["id"].as_str().unwrap());
    assert_eq!(
        request(&c, "PUT", &path, multipart(&[("image", &png())]))
            .await
            .status(),
        StatusCode::OK
    );
    let response = request(&c, "PUT", &path, multipart(&[("image", &png())])).await;
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(response.headers().contains_key(header::RETRY_AFTER));
    assert_eq!(value(response).await["code"], "rate_limited");
    assert_eq!(t_state(&c).image_processing.available_permits(), 2);
    assert!(
        t_state(&c)
            .ai_limiter
            .check("unrelated".into(), std::time::Instant::now())
            .is_ok()
    );
}

fn t_state(c: &Client) -> &crate::AppState {
    &c._t.as_ref().unwrap().state
}

#[tokio::test]
async fn repository_last_commit_wins_rollback_concurrent_edits_and_cascade() {
    use application::ports::NormalizedVendorImage;
    let c = client().await;
    let (_, vendor) = c.post("/vendors", json!({"name":"Transactions"})).await;
    let id = vendor["id"].as_str().unwrap().parse().unwrap();
    let actor = c.get("/me").await.1["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let state = t_state(&c);
    let repo = state.directory.vendors.clone();
    let image = || infrastructure::vendor_image::normalize(&png()).unwrap();
    let now = std::time::SystemTime::now();
    let first = repo.put_image(actor, id, image(), now).await.unwrap();
    // SQL constraint failure must roll back both image and parent timestamp.
    assert!(
        repo.put_image(
            actor,
            id,
            NormalizedVendorImage {
                data: vec![],
                width: 1,
                height: 1
            },
            now + std::time::Duration::from_secs(60)
        )
        .await
        .is_err()
    );
    assert_eq!(repo.read_image(actor, id).await.unwrap().metadata, first);
    let before = repo.find(actor, id).await.unwrap().unwrap();
    assert_eq!(before.updated_at, first.updated_at);
    let (a, b) = tokio::join!(
        repo.put_image(actor, id, image(), now),
        repo.put_image(actor, id, image(), now)
    );
    let (a, b) = (a.unwrap(), b.unwrap());
    assert_ne!(a.revision, b.revision);
    let current = repo.read_image(actor, id).await.unwrap();
    assert!(current.metadata == a || current.metadata == b);
    let (put, edit) = tokio::join!(
        repo.put_image(actor, id, image(), now),
        state.directory.edit_vendor(
            actor,
            id,
            domain::directory::VendorInfoEdit {
                note: domain::shared::Patch::Set("concurrent".into()),
                ..Default::default()
            }
        )
    );
    put.unwrap();
    edit.unwrap();
    let vendor = repo.find(actor, id).await.unwrap().unwrap();
    assert_eq!(vendor.value.note(), Some("concurrent"));
    assert!(vendor.value.image.is_some());
    let (put, delete) = tokio::join!(
        repo.put_image(actor, id, image(), now),
        repo.delete_image(actor, id, now)
    );
    put.unwrap();
    delete.unwrap();
    if let Ok(current) = repo.read_image(actor, id).await {
        assert_eq!(current.data.len(), current.metadata.size_bytes as usize);
    }
    repo.delete_image(actor, id, now).await.unwrap();
    let deleted_at = repo.find(actor, id).await.unwrap().unwrap().updated_at;
    repo.delete_image(actor, id, now + std::time::Duration::from_secs(60))
        .await
        .unwrap();
    assert_eq!(
        repo.find(actor, id).await.unwrap().unwrap().updated_at,
        deleted_at
    );
    repo.put_image(actor, id, image(), now).await.unwrap();
    sqlx::query("DELETE FROM organizers WHERE id=?")
        .bind(actor.to_string())
        .execute(&state.db.write)
        .await
        .unwrap();
    let (count,): (i64,) = sqlx::query_as("SELECT count(*) FROM vendor_images")
        .fetch_one(&state.db.read)
        .await
        .unwrap();
    assert_eq!(count, 0);
}
