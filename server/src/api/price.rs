use axum::{
    extract::{Query, State},
    Json,
};
use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;

use crate::auth::ExtractUser;
use crate::error::{AppError, AppResult};

// ── GET /api/price-history ──────────────────────────────────────────────────

#[derive(Debug, Deserialize, Default)]
pub struct PriceHistoryQuery {
    pub product_id: Option<Uuid>,
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
}

#[derive(Debug, Serialize)]
pub struct PricePoint {
    pub transaction_id: Uuid,
    pub occurred_on: NaiveDate,
    pub unit_price: Decimal,
    pub quantity: Option<Decimal>,
    pub line_amount: Decimal,
    pub merchant_id: Option<Uuid>,
    pub merchant_name: Option<String>,
    pub memo: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PriceHistoryResponse {
    pub product_id: Uuid,
    pub product_name: String,
    pub merchant_id: Option<Uuid>,
    pub merchant_name: Option<String>,
    pub points: Vec<PricePoint>,
    /// Total points returned (= points.len()).
    pub total: usize,
    /// min/max/avg unit_price across the returned points (null when empty).
    pub min_unit_price: Option<Decimal>,
    pub max_unit_price: Option<Decimal>,
    pub avg_unit_price: Option<Decimal>,
}

/// GET /api/price-history?product_id=&from=&to=
///
/// Unit-price time series for a single product. Only memo-bearing transactions
/// (with `product_id` set and `unit_price` non-null) appear here. Returns 400
/// when `product_id` is missing.
pub async fn handle_get_price_history(
    State(pool): State<Arc<PgPool>>,
    ExtractUser(user): ExtractUser,
    Query(q): Query<PriceHistoryQuery>,
) -> AppResult<Json<PriceHistoryResponse>> {
    let owner_id = user.sub;
    let product_id = q
        .product_id
        .ok_or_else(|| AppError::BadRequest("product_id is required".into()))?;

    let product = sqlx::query!(
        r#"
        SELECT
            p.id          AS "id!: Uuid",
            p.name        AS "name!: String",
            p.merchant_id AS "merchant_id?: Uuid",
            m.name        AS "merchant_name?: String"
        FROM products p
        LEFT JOIN merchants m ON m.id = p.merchant_id AND m.owner_id = p.owner_id
        WHERE p.owner_id = $1 AND p.id = $2
        "#,
        owner_id,
        product_id,
    )
    .fetch_optional(&*pool)
    .await?
    .ok_or_else(|| AppError::NotFound(format!("product {product_id}")))?;

    let rows = sqlx::query!(
        r#"
        SELECT
            t.id           AS "id!: Uuid",
            t.occurred_on  AS "occurred_on!: NaiveDate",
            t.unit_price   AS "unit_price!: Decimal",
            t.quantity     AS "quantity?: Decimal",
            t.amount       AS "amount!: Decimal",
            t.merchant_id  AS "merchant_id?: Uuid",
            m.name         AS "merchant_name?: String",
            t.memo
        FROM transactions t
        LEFT JOIN merchants m ON m.id = t.merchant_id AND m.owner_id = t.owner_id
        WHERE t.owner_id = $1
          AND t.product_id = $2
          AND t.unit_price IS NOT NULL
          AND ($3::date IS NULL OR t.occurred_on >= $3)
          AND ($4::date IS NULL OR t.occurred_on <= $4)
        ORDER BY t.occurred_on, t.id
        "#,
        owner_id,
        product_id,
        q.from as Option<NaiveDate>,
        q.to as Option<NaiveDate>,
    )
    .fetch_all(&*pool)
    .await?;

    let points: Vec<PricePoint> = rows
        .into_iter()
        .map(|r| PricePoint {
            transaction_id: r.id,
            occurred_on: r.occurred_on,
            unit_price: r.unit_price,
            quantity: r.quantity,
            line_amount: -r.amount,
            merchant_id: r.merchant_id,
            merchant_name: r.merchant_name,
            memo: r.memo,
        })
        .collect();

    let (min, max, avg) = if points.is_empty() {
        (None, None, None)
    } else {
        let prices: Vec<Decimal> = points.iter().map(|p| p.unit_price).collect();
        let mn = prices.iter().copied().min().unwrap();
        let mx = prices.iter().copied().max().unwrap();
        let sum: Decimal = prices.iter().copied().sum();
        let avg = sum / Decimal::from(prices.len() as i64);
        (Some(mn), Some(mx), Some(avg))
    };

    Ok(Json(PriceHistoryResponse {
        product_id: product.id,
        product_name: product.name,
        merchant_id: product.merchant_id,
        merchant_name: product.merchant_name,
        total: points.len(),
        points,
        min_unit_price: min,
        max_unit_price: max,
        avg_unit_price: avg,
    }))
}
