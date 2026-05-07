use axum::{
    extract::{Path, State},
    Json,
};
use rust_decimal::Decimal;
use serde::Serialize;
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;

use crate::auth::ExtractUser;
use crate::error::AppResult;

#[derive(Debug, Serialize)]
pub struct IncomeByActor {
    pub actor_id: Option<Uuid>,
    pub actor_name: String,
    pub total: Decimal,
}

#[derive(Debug, Serialize)]
pub struct IncomeResponse {
    /// "YYYY-MM" 형식.
    pub month: String,
    pub by_actor: Vec<IncomeByActor>,
    pub total: Decimal,
}

/// GET /api/summary/income/:year/:month
///
/// 해당 월의 `kind='income'` 카테고리 트랜잭션을 액터별로 합산한다.
/// 저장 규약상 수입은 양수이므로 그대로 SUM(amount).
/// 등록된 모든 액터를 결과에 포함하되 거래 없는 액터는 total=0 으로 채운다.
pub async fn handle_get_income(
    State(pool): State<Arc<PgPool>>,
    ExtractUser(user): ExtractUser,
    Path((year, month)): Path<(i32, i32)>,
) -> AppResult<Json<IncomeResponse>> {
    let owner_id = user.sub;

    let rows = sqlx::query!(
        r#"
        SELECT
            a.id   AS "actor_id!: Uuid",
            a.name AS "actor_name!: String",
            COALESCE(
                (SELECT SUM(t.amount)
                 FROM transactions t
                 JOIN categories c ON c.id = t.category_id AND c.owner_id = t.owner_id
                 WHERE t.owner_id = $1
                   AND t.actor_id = a.id
                   AND c.kind = 'income'
                   AND t.occurred_on >= make_date($2, $3, 1)
                   AND t.occurred_on  < make_date($2, $3, 1) + INTERVAL '1 month'),
                0::numeric(15,2)
            ) AS "total!: Decimal"
        FROM ledger_actors a
        WHERE a.owner_id = $1
        ORDER BY a.name
        "#,
        owner_id,
        year,
        month,
    )
    .fetch_all(&*pool)
    .await?;

    let by_actor: Vec<IncomeByActor> = rows
        .into_iter()
        .map(|r| IncomeByActor {
            actor_id: Some(r.actor_id),
            actor_name: r.actor_name,
            total: r.total,
        })
        .collect();

    let total: Decimal = by_actor.iter().map(|e| e.total).sum();

    Ok(Json(IncomeResponse {
        month: format!("{:04}-{:02}", year, month),
        by_actor,
        total,
    }))
}
