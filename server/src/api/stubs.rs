/// Unimplemented API stubs (M3 — to be replaced in later steps).
/// All return 501 Not Implemented.
///
/// M2 Step B stubs (handle_aliases, handle_review_queue) have been removed —
/// they are now implemented in `api/aliases.rs`.
use axum::{http::StatusCode, response::IntoResponse, Json};
use serde_json::json;

pub async fn handle_price_history() -> impl IntoResponse {
    // TODO M3: GET /api/price-history?product_id= — unit-price time series
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({ "detail": "Not implemented" })),
    )
}

pub async fn handle_products() -> impl IntoResponse {
    // TODO M3: GET /api/products?merchant_id=&q= — product list/search
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({ "detail": "Not implemented" })),
    )
}

pub async fn handle_merchant_stats() -> impl IntoResponse {
    // TODO M3: GET /api/merchant-stats?merchant_id= — monthly merchant totals
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({ "detail": "Not implemented" })),
    )
}
