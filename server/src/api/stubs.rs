/// 미구현 API 스텁 모음 (M2/M3에서 구현 예정)
/// 모두 501 Not Implemented 반환
use axum::{extract::Path, http::StatusCode, response::IntoResponse, Json};
use serde_json::json;

pub async fn handle_summary(Path((_year, _month)): Path<(i32, i32)>) -> impl IntoResponse {
    // TODO M2: GET /api/summary/:year/:month — 카테고리×액터 피벗
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({ "detail": "Not implemented" })),
    )
}

pub async fn handle_settlement(Path((_year, _month)): Path<(i32, i32)>) -> impl IntoResponse {
    // TODO M2: GET /api/settlement/:year/:month — v_monthly_settlement 조회
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({ "detail": "Not implemented" })),
    )
}

pub async fn handle_price_history() -> impl IntoResponse {
    // TODO M3: GET /api/price-history?product_id= — 단가 시계열
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({ "detail": "Not implemented" })),
    )
}

pub async fn handle_products() -> impl IntoResponse {
    // TODO M3: GET /api/products?merchant_id=&q= — 상품 목록
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({ "detail": "Not implemented" })),
    )
}

pub async fn handle_merchant_stats() -> impl IntoResponse {
    // TODO M3: GET /api/merchant-stats?merchant_id= — 구매처 월별 통계
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({ "detail": "Not implemented" })),
    )
}

pub async fn handle_aliases() -> impl IntoResponse {
    // TODO M2: GET/POST/DELETE /api/aliases — 별칭 관리
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({ "detail": "Not implemented" })),
    )
}

pub async fn handle_review_queue() -> impl IntoResponse {
    // TODO M2: GET /api/review-queue — 미매칭 raw 텍스트 목록
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({ "detail": "Not implemented" })),
    )
}

pub async fn handle_categories() -> impl IntoResponse {
    // TODO M2: GET /api/categories — 카테고리 목록
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({ "detail": "Not implemented" })),
    )
}

pub async fn handle_merchants() -> impl IntoResponse {
    // TODO M2: GET /api/merchants — 구매처 목록
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({ "detail": "Not implemented" })),
    )
}

pub async fn handle_payment_methods() -> impl IntoResponse {
    // TODO M2: GET /api/payment-methods — 결제수단 목록
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({ "detail": "Not implemented" })),
    )
}
