/// Integration tests for PATCH /api/categories/:id/kind
///
/// Verifies kind flipping, invalid-value rejection, and 차감 protection.

mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
    middleware, routing, Router,
};
use finance_manager::auth::AuthUser;
use sea_orm::{
    ConnectionTrait, DatabaseBackend, DatabaseConnection, FromQueryResult, Statement,
};
use std::sync::Arc;
use tower::ServiceExt;
use uuid::Uuid;

// ── Router ────────────────────────────────────────────────────────────────────

fn build_test_router(db: Arc<DatabaseConnection>, owner_id: Uuid) -> Router {
    let user = AuthUser {
        sub: owner_id,
        email: "test@example.com".to_string(),
        groups: vec![],
    };

    Router::new()
        .route(
            "/api/categories/:id/kind",
            routing::patch(finance_manager::api::categories::handle_patch_category_kind),
        )
        .with_state(db)
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

// ── Helpers ───────────────────────────────────────────────────────────────────

#[derive(FromQueryResult)]
struct IdRow { id: Uuid }

#[derive(FromQueryResult)]
struct KindRow { kind: String }

// ── Tests ─────────────────────────────────────────────────────────────────────

/// Happy path: create a category with kind='expense', PATCH to 'income', assert 200 + DB updated.
#[tokio::test]
async fn patch_category_kind_flips_value() {
    let t = common::TestDb::new().await;
    let owner_id = Uuid::new_v4();
    let db = Arc::clone(&t.db);

    // Create a category with kind='expense'
    let category_id: Uuid = IdRow::find_by_statement(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO categories (owner_id, name, kind) VALUES ($1, '식비', 'expense') RETURNING id",
        [owner_id.into()],
    ))
    .one(&*t.db)
    .await
    .unwrap()
    .unwrap()
    .id;

    let app = build_test_router(Arc::clone(&db), owner_id);

    let body = serde_json::to_vec(&serde_json::json!({"kind": "income"})).unwrap();
    let response = app
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(format!("/api/categories/{}/kind", category_id))
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

    assert_eq!(body["kind"], "income");

    // Verify the DB row was actually updated
    let db_kind = KindRow::find_by_statement(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT kind FROM categories WHERE id = $1 AND owner_id = $2",
        [category_id.into(), owner_id.into()],
    ))
    .one(&*t.db)
    .await
    .unwrap()
    .unwrap()
    .kind;

    assert_eq!(db_kind, "income");
}

/// Invalid kind value should return 400 or 422.
#[tokio::test]
async fn patch_category_kind_rejects_invalid_value() {
    let t = common::TestDb::new().await;
    let owner_id = Uuid::new_v4();
    let db = Arc::clone(&t.db);

    // Create a category to target
    let category_id: Uuid = IdRow::find_by_statement(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO categories (owner_id, name, kind) VALUES ($1, '식비', 'expense') RETURNING id",
        [owner_id.into()],
    ))
    .one(&*t.db)
    .await
    .unwrap()
    .unwrap()
    .id;

    let app = build_test_router(db, owner_id);

    let body = serde_json::to_vec(&serde_json::json!({"kind": "rubbish"})).unwrap();
    let response = app
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(format!("/api/categories/{}/kind", category_id))
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();

    // axum JSON deserialization may return 422; our handler returns 400; either is acceptable
    assert!(
        response.status() == StatusCode::BAD_REQUEST
            || response.status() == StatusCode::UNPROCESSABLE_ENTITY,
        "expected 400 or 422, got {}",
        response.status()
    );
}

/// 차감 is a protected system category — PATCH to change its kind must return 409.
#[tokio::test]
async fn patch_category_kind_protects_deduction() {
    let t = common::TestDb::new().await;
    let owner_id = Uuid::new_v4();
    let db = Arc::clone(&t.db);

    // Create a 차감 category
    let category_id: Uuid = IdRow::find_by_statement(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO categories (owner_id, name, kind) VALUES ($1, '차감', 'expense') RETURNING id",
        [owner_id.into()],
    ))
    .one(&*t.db)
    .await
    .unwrap()
    .unwrap()
    .id;

    let app = build_test_router(db, owner_id);

    let body = serde_json::to_vec(&serde_json::json!({"kind": "income"})).unwrap();
    let response = app
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(format!("/api/categories/{}/kind", category_id))
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CONFLICT);
}
