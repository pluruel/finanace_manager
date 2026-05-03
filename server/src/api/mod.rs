pub mod aliases;
pub mod categories;
pub mod export;
pub mod import;
pub mod merchant_stats;
pub mod price;
pub mod products;
pub mod settlement;
pub mod summary;
pub mod transactions;

use axum::{
    extract::DefaultBodyLimit,
    middleware,
    routing::{delete, get, post},
    Router,
};
use sqlx::PgPool;
use std::sync::Arc;

use crate::auth::{auth_middleware, JwksClient};

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
        // M4-B: xlsx export
        .route("/api/export/:year/:month", get(export::handle_get_export))
        // M3: price tracking + merchant stats
        .route("/api/price-history", get(price::handle_get_price_history))
        .route("/api/products", get(products::handle_get_products))
        .route(
            "/api/merchant-stats",
            get(merchant_stats::handle_get_merchant_stats),
        )
        // M2 Step B: alias CRUD
        .route("/api/aliases", post(aliases::handle_post_alias))
        .route("/api/aliases/:id", delete(aliases::handle_delete_alias))
        .route("/api/review-queue", get(aliases::handle_get_review_queue))
        // M2 Step B: entity confirm
        .route(
            "/api/entities/:scope/:id/confirm",
            post(aliases::handle_confirm_entity),
        )
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
