use axum::{
    extract::{Path, State},
    Json,
};
use rust_decimal::Decimal;
use sea_orm::DatabaseConnection;
use serde::Serialize;
use std::sync::Arc;

use crate::auth::ExtractUser;
use crate::error::AppResult;

#[derive(Debug, Serialize)]
pub struct SettlementResponse {
    pub year: i32,
    pub month: i32,
    /// Sum of 공동-actor, non-차감, sign=1 transactions.
    pub recognized_expense: Decimal,
    /// Sum of 차감-category transactions.
    pub deducted_amount: Decimal,
    /// recognized_expense - deducted_amount.
    pub settlement_input: Decimal,
}

/// GET /api/settlement/:year/:month
///
/// Queries v_monthly_settlement for the requested month.
/// Returns zeros (not 404) when no data exists for that month.
pub async fn handle_get_settlement(
    State(db): State<Arc<DatabaseConnection>>,
    ExtractUser(user): ExtractUser,
    Path((year, month)): Path<(i32, i32)>,
) -> AppResult<Json<SettlementResponse>> {
    let pool = crate::db::pool_of(&db);
    let owner_id = user.sub;

    // v_monthly_settlement groups by date_trunc('month', occurred_on)::date.
    // We match on the first day of the requested month.
    let row = sqlx::query!(
        r#"
        SELECT
            recognized_expense AS "recognized_expense!: Decimal",
            deducted_amount    AS "deducted_amount!: Decimal",
            settlement_input   AS "settlement_input!: Decimal"
        FROM v_monthly_settlement
        WHERE owner_id = $1
          AND month = make_date($2, $3, 1)
        "#,
        owner_id,
        year,
        month,
    )
    .fetch_optional(pool)
    .await?;

    // If no data for that month, return zeros rather than 404.
    let (recognized_expense, deducted_amount, settlement_input) = match row {
        Some(r) => (r.recognized_expense, r.deducted_amount, r.settlement_input),
        None => (Decimal::ZERO, Decimal::ZERO, Decimal::ZERO),
    };

    Ok(Json(SettlementResponse {
        year,
        month,
        recognized_expense,
        deducted_amount,
        settlement_input,
    }))
}
