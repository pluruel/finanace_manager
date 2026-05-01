/// 테스트 5 (추가): body limit 초과 → 413 검증
///
/// 20MB 제한을 낮게 설정한 별도 라우터를 구성해 body 초과 시 413이 반환되는지 확인한다.

use axum::{
    body::Body,
    extract::DefaultBodyLimit,
    http::{Request, StatusCode},
    middleware, routing, Router,
};
use finance_manager::auth::AuthUser;
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

fn build_import_router_with_limit(
    pool: std::sync::Arc<PgPool>,
    owner_id: Uuid,
    limit_bytes: usize,
) -> Router {
    let user = AuthUser {
        sub: owner_id,
        email: "test@example.com".to_string(),
        groups: vec![],
    };

    let import_route = Router::new()
        .route(
            "/api/import",
            routing::post(finance_manager::api::import::handle_import),
        )
        .layer(DefaultBodyLimit::max(limit_bytes));

    import_route
        .with_state(pool)
        .layer(middleware::from_fn(
            move |mut req: axum::http::Request<Body>, next: middleware::Next| {
                let user = user.clone();
                async move {
                    req.extensions_mut().insert(user);
                    next.run(req).await
                }
            },
        ))
}

/// body limit 초과 시 413 반환 검증 테스트
///
/// `DefaultBodyLimit::max(limit_bytes)` 초과 시 axum의 `MultipartError::status()`가
/// PAYLOAD_TOO_LARGE(413)를 반환하고, handle_import가 이를 `AppError::PayloadTooLarge`로
/// 변환해 413을 응답해야 한다.
#[sqlx::test(migrations = "./migrations")]
async fn import_body_too_large_returns_413(pool: PgPool) {
    let pool = std::sync::Arc::new(pool);
    let owner_id = Uuid::new_v4();

    // 512바이트 제한으로 라우터 구성
    let app = build_import_router_with_limit(pool, owner_id, 512);

    // 유효한 멀티파트 형식이되 크기를 초과하는 body 전송
    let boundary = "testboundary";
    // 800바이트 데이터 (제한 512B 초과)
    let large_data = vec![b'A'; 800];
    let mut body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"test.xlsx\"\r\nContent-Type: application/vnd.openxmlformats-officedocument.spreadsheetml.sheet\r\n\r\n"
    ).into_bytes();
    body.extend_from_slice(&large_data);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

    let req = Request::builder()
        .method("POST")
        .uri("/api/import")
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();

    assert_eq!(
        resp.status(),
        StatusCode::PAYLOAD_TOO_LARGE,
        "body limit 초과 시 413 Payload Too Large이어야 한다, got {}",
        resp.status()
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn import_empty_body_returns_400(pool: PgPool) {
    let pool = std::sync::Arc::new(pool);
    let owner_id = Uuid::new_v4();

    // 충분한 limit으로 라우터 구성
    let app = build_import_router_with_limit(pool, owner_id, 20 * 1024 * 1024);

    // 빈 멀티파트 전송
    let boundary = "----testboundary";
    let empty_multipart = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"test.xlsx\"\r\nContent-Type: application/vnd.openxmlformats-officedocument.spreadsheetml.sheet\r\n\r\n\r\n--{boundary}--\r\n"
    );

    let req = Request::builder()
        .method("POST")
        .uri("/api/import")
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(empty_multipart))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();

    // 빈 파일 → 400 Bad Request
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "빈 파일 업로드 시 400이어야 한다, got {}",
        resp.status()
    );
}
