use axum::{
    extract::{Query, State},
    Json,
};
use chrono::NaiveDate;
use rust_decimal::Decimal;
use sea_orm::{DatabaseConnection, DbBackend, FromQueryResult, Statement};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use crate::auth::ExtractUser;
use crate::error::{AppError, AppResult};

// ── GET /api/merchant-stats ─────────────────────────────────────────────────

#[derive(Debug, Deserialize, Default)]
pub struct MerchantStatsQuery {
    pub merchant_id: Option<Uuid>,
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
    /// When true, restrict to memo-less rows (product_id IS NULL). Default false.
    #[serde(default)]
    pub memo_less_only: bool,
}

#[derive(Debug, Serialize)]
pub struct MonthlyMerchantPoint {
    /// First-of-month date.
    pub month: NaiveDate,
    /// `-SUM(t.amount)` over expense rows for the (merchant, month) cell — positive expense magnitude.
    pub total: Decimal,
    /// Count of transactions in the cell.
    pub transaction_count: i64,
    /// Count of memo-less transactions (product_id IS NULL) in the cell.
    pub memo_less_count: i64,
}

#[derive(Debug, Serialize)]
pub struct MerchantStatsResponse {
    pub merchant_id: Uuid,
    pub merchant_name: String,
    pub points: Vec<MonthlyMerchantPoint>,
    /// Sum of `point.total` across the returned range.
    pub grand_total: Decimal,
    pub transaction_count: i64,
    pub memo_less_count: i64,
}

#[derive(FromQueryResult)]
struct MerchantRow {
    id: Uuid,
    name: String,
}

#[derive(FromQueryResult)]
struct MerchantStatsRow {
    month: NaiveDate,
    total: Decimal,
    tx_count: i64,
    memo_less: i64,
}

/// GET /api/merchant-stats?merchant_id=&from=&to=&memo_less_only=
///
/// Monthly per-merchant totals. Used as a fallback for memo-less transactions
/// where unit-price tracking isn't possible (PLAN §6 M3 acceptance criteria —
/// the 167 memo-less Feb rows are surfaced here).
pub async fn handle_get_merchant_stats(
    State(db): State<Arc<DatabaseConnection>>,
    ExtractUser(user): ExtractUser,
    Query(q): Query<MerchantStatsQuery>,
) -> AppResult<Json<MerchantStatsResponse>> {
    let owner_id = user.sub;
    let merchant_id = q
        .merchant_id
        .ok_or_else(|| AppError::BadRequest("merchant_id is required".into()))?;

    let merchant = MerchantRow::find_by_statement(Statement::from_sql_and_values(
        DbBackend::Postgres,
        r#"
        SELECT id, name
        FROM merchants
        WHERE owner_id = $1 AND id = $2
        "#,
        [owner_id.into(), merchant_id.into()],
    ))
    .one(&*db)
    .await?
    .ok_or_else(|| AppError::NotFound(format!("merchant {merchant_id}")))?;

    let rows = MerchantStatsRow::find_by_statement(Statement::from_sql_and_values(
        DbBackend::Postgres,
        r#"
        SELECT
            date_trunc('month', t.occurred_on)::date     AS month,
            (-SUM(t.amount))::numeric(15,2)              AS total,
            COUNT(*)                                     AS tx_count,
            COUNT(*) FILTER (WHERE t.product_id IS NULL) AS memo_less
        FROM transactions t
        JOIN categories c ON c.id = t.category_id AND c.owner_id = t.owner_id
        WHERE t.owner_id = $1
          AND t.merchant_id = $2
          AND c.kind = 'expense'
          AND ($3::date IS NULL OR t.occurred_on >= $3)
          AND ($4::date IS NULL OR t.occurred_on <= $4)
          AND ($5 = false OR t.product_id IS NULL)
        GROUP BY date_trunc('month', t.occurred_on)
        ORDER BY 1
        "#,
        [
            owner_id.into(),
            merchant_id.into(),
            q.from.into(),
            q.to.into(),
            q.memo_less_only.into(),
        ],
    ))
    .all(&*db)
    .await?;

    let points: Vec<MonthlyMerchantPoint> = rows
        .into_iter()
        .map(|r| MonthlyMerchantPoint {
            month: r.month,
            total: r.total,
            transaction_count: r.tx_count,
            memo_less_count: r.memo_less,
        })
        .collect();

    let grand_total: Decimal = points.iter().map(|p| p.total).sum();
    let transaction_count: i64 = points.iter().map(|p| p.transaction_count).sum();
    let memo_less_count: i64 = points.iter().map(|p| p.memo_less_count).sum();

    Ok(Json(MerchantStatsResponse {
        merchant_id: merchant.id,
        merchant_name: merchant.name,
        points,
        grand_total,
        transaction_count,
        memo_less_count,
    }))
}
