mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
    middleware, routing, Router,
};
use finance_manager::auth::AuthUser;
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use std::sync::Arc;
use tower::ServiceExt;
use uuid::Uuid;

fn build_router(db: Arc<DatabaseConnection>, owner_id: Uuid) -> Router {
    let user = AuthUser {
        sub: owner_id,
        email: "test@example.com".to_string(),
        groups: vec![],
    };

    Router::new()
        .route(
            "/api/payment-methods/:id/actor",
            routing::patch(finance_manager::api::categories::handle_patch_payment_method_actor),
        )
        .with_state(db)
        .layer(middleware::from_fn(
            move |mut req: Request<Body>, next: middleware::Next| {
                let user = user.clone();
                async move {
                    req.extensions_mut().insert(user);
                    next.run(req).await
                }
            },
        ))
}

async fn seed_actor_and_pm(
    db: &Arc<DatabaseConnection>,
    owner_id: Uuid,
) -> (Uuid, Uuid) {
    // Returns (actor_id, pm_id)
    let actor_id = Uuid::new_v4();
    let pm_id = Uuid::new_v4();

    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO ledger_actors (id, owner_id, name) VALUES ($1, $2, '아기')",
        [actor_id.into(), owner_id.into()],
    ))
    .await
    .unwrap();

    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO payment_methods (id, owner_id, name, review_state) VALUES ($1, $2, '농협', 'pending')",
        [pm_id.into(), owner_id.into()],
    ))
    .await
    .unwrap();

    (actor_id, pm_id)
}

#[tokio::test]
async fn patch_actor_assigns_actor_to_payment_method() {
    let t = common::TestDb::new().await;
    let owner_id = Uuid::new_v4();
    let (actor_id, pm_id) = seed_actor_and_pm(&t.db, owner_id).await;

    let app = build_router(Arc::clone(&t.db), owner_id);
    let body = serde_json::json!({ "actor_id": actor_id });
    let req = Request::builder()
        .method("PATCH")
        .uri(format!("/api/payment-methods/{pm_id}/actor"))
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(json["id"].as_str().unwrap(), pm_id.to_string());
    assert_eq!(json["actor_id"].as_str().unwrap(), actor_id.to_string());
    assert_eq!(json["name"].as_str().unwrap(), "농협");
}

#[tokio::test]
async fn patch_actor_returns_404_for_unknown_payment_method() {
    let t = common::TestDb::new().await;
    let owner_id = Uuid::new_v4();
    let (actor_id, _pm_id) = seed_actor_and_pm(&t.db, owner_id).await;

    let unknown_pm_id = Uuid::new_v4();
    let app = build_router(Arc::clone(&t.db), owner_id);
    let body = serde_json::json!({ "actor_id": actor_id });
    let req = Request::builder()
        .method("PATCH")
        .uri(format!("/api/payment-methods/{unknown_pm_id}/actor"))
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn patch_actor_returns_400_for_unknown_actor() {
    let t = common::TestDb::new().await;
    let owner_id = Uuid::new_v4();
    let (_actor_id, pm_id) = seed_actor_and_pm(&t.db, owner_id).await;

    let unknown_actor_id = Uuid::new_v4();
    let app = build_router(Arc::clone(&t.db), owner_id);
    let body = serde_json::json!({ "actor_id": unknown_actor_id });
    let req = Request::builder()
        .method("PATCH")
        .uri(format!("/api/payment-methods/{pm_id}/actor"))
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}
