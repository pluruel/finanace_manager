pub mod categories;
pub mod import;
pub mod settlement;
pub mod stubs;
pub mod summary;
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

/// Full API router.
pub fn router(pool: Arc<PgPool>, jwks: Arc<JwksClient>) -> Router {
    // /api/import allows up to 20 MB; all other routes use the axum default (2 MB).
    let import_route = Router::new()
        .route("/api/import", post(import::handle_import))
        .layer(DefaultBodyLimit::max(20 * 1024 * 1024));

    let protected = Router::new()
        .merge(import_route)
        .route("/api/transactions", get(transactions::handle_get_transactions))
        .route("/api/summary/:year/:month", get(summary::handle_get_summary))
        .route("/api/settlement/:year/:month", get(settlement::handle_get_settlement))
        .route("/api/price-history", get(handle_price_history))
        .route("/api/products", get(handle_products))
        .route("/api/merchant-stats", get(handle_merchant_stats))
        .route("/api/aliases", get(handle_aliases).post(handle_aliases).delete(handle_aliases))
        .route("/api/review-queue", get(handle_review_queue))
        .route("/api/categories", get(categories::handle_get_categories))
        .route("/api/merchants", get(categories::handle_get_merchants))
        .route("/api/payment-methods", get(categories::handle_get_payment_methods))
        .with_state(pool.clone())
        .layer(middleware::from_fn_with_state(jwks, auth_middleware));

    Router::new()
        .route("/health", get(health))
        .merge(protected)
}

async fn health() -> &'static str {
    "ok"
}
