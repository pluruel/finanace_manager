use axum::{
    extract::{Query, State},
    Json,
};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;

use crate::auth::ExtractUser;
use crate::error::AppResult;
use crate::import::normalize::to_norm_key;

// ── GET /api/products ────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Default)]
pub struct ProductQuery {
    pub merchant_id: Option<Uuid>,
    pub q: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ProductItem {
    pub id: Uuid,
    pub name: String,
    pub merchant_id: Option<Uuid>,
    pub merchant_name: Option<String>,
    pub review_state: String,
    pub transaction_count: i64,
}

/// GET /api/products?merchant_id=&q=
///
/// List/search products joined with their merchant. Includes a transaction_count
/// per product so the UI can prioritize products with actual data.
pub async fn handle_get_products(
    State(pool): State<Arc<PgPool>>,
    ExtractUser(user): ExtractUser,
    Query(q): Query<ProductQuery>,
) -> AppResult<Json<Vec<ProductItem>>> {
    let owner_id = user.sub;
    let merchant_filter = q.merchant_id;
    let name_filter: Option<String> = q
        .q
        .as_deref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(to_norm_key);

    let rows = sqlx::query!(
        r#"
        SELECT
            p.id           AS "id!: Uuid",
            p.name         AS "name!: String",
            p.merchant_id  AS "merchant_id?: Uuid",
            m.name         AS "merchant_name?: String",
            p.review_state AS "review_state!: String",
            (SELECT COUNT(*) FROM transactions t
             WHERE t.owner_id = p.owner_id AND t.product_id = p.id) AS "transaction_count!: i64"
        FROM products p
        LEFT JOIN merchants m ON m.id = p.merchant_id AND m.owner_id = p.owner_id
        WHERE p.owner_id = $1
          AND ($2::uuid IS NULL OR p.merchant_id = $2)
          AND ($3::text IS NULL
               OR position($3 in regexp_replace(lower(p.name), '_', ' ', 'g')) > 0)
        ORDER BY p.name
        "#,
        owner_id,
        merchant_filter,
        name_filter,
    )
    .fetch_all(&*pool)
    .await?;

    let items = rows
        .into_iter()
        .map(|r| ProductItem {
            id: r.id,
            name: r.name,
            merchant_id: r.merchant_id,
            merchant_name: r.merchant_name,
            review_state: r.review_state,
            transaction_count: r.transaction_count,
        })
        .collect();

    Ok(Json(items))
}
