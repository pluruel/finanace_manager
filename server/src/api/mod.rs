pub mod import;
pub mod stubs;
pub mod transactions;

use axum::{
    extract::DefaultBodyLimit,
    middleware,
    routing::{get, post},
    Router,
};
use sqlx::PgPool;
use std::sync::Arc;

use crate::auth::{auth_middleware, JwksClient};
use stubs::*;

/// 전체 API 라우터 구성
pub fn router(pool: Arc<PgPool>, jwks: Arc<JwksClient>) -> Router {
    // import 라우트는 20MB까지 허용 (xlsx 파일 크기 대응)
    // 다른 라우트는 axum 기본값(2MB) 유지
    let import_route = Router::new()
        .route("/api/import", post(import::handle_import))
        .layer(DefaultBodyLimit::max(20 * 1024 * 1024));

    let protected = Router::new()
        .merge(import_route)
        .route("/api/transactions", get(transactions::handle_get_transactions))
        .route("/api/summary/:year/:month", get(handle_summary))
        .route("/api/settlement/:year/:month", get(handle_settlement))
        .route("/api/price-history", get(handle_price_history))
        .route("/api/products", get(handle_products))
        .route("/api/merchant-stats", get(handle_merchant_stats))
        .route("/api/aliases", get(handle_aliases).post(handle_aliases).delete(handle_aliases))
        .route("/api/review-queue", get(handle_review_queue))
        .route("/api/categories", get(handle_categories))
        .route("/api/merchants", get(handle_merchants))
        .route("/api/payment-methods", get(handle_payment_methods))
        .with_state(pool.clone())
        .layer(middleware::from_fn_with_state(jwks, auth_middleware));

    Router::new()
        .route("/health", get(health))
        .merge(protected)
}

async fn health() -> &'static str {
    "ok"
}
