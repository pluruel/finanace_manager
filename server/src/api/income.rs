use axum::{
    extract::{Path, State},
    Json,
};
use rust_decimal::Decimal;
use sea_orm::{DatabaseConnection, DbBackend, FromQueryResult, Statement};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

use crate::api::summary::{ByActorEntry, CategorySummary};
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
    /// 수입 카테고리 × 액터 분해. expense summary 와 동일 구조이나 amount 는 부호 그대로 양수.
    pub categories: Vec<CategorySummary>,
}

#[derive(FromQueryResult)]
struct IncomeByActorRow {
    actor_id: Uuid,
    actor_name: String,
    total: Decimal,
}

#[derive(FromQueryResult)]
struct IncomeCatRow {
    category_id: Uuid,
    category_name: String,
    kind: String,
    actor_id: Option<Uuid>,
    actor_name: Option<String>,
    amount: Decimal,
}

/// GET /api/summary/income/:year/:month
///
/// 해당 월의 `kind='income'` 카테고리 트랜잭션을 액터별로 합산한다.
/// 저장 규약상 수입은 양수이므로 그대로 SUM(amount).
/// 등록된 모든 액터를 by_actor 에 포함하되 거래 없는 액터는 total=0 으로 채운다.
/// `categories` 는 income kind 카테고리만 포함하며 expense summary 와 동일 셰이프.
pub async fn handle_get_income(
    State(db): State<Arc<DatabaseConnection>>,
    ExtractUser(user): ExtractUser,
    Path((year, month)): Path<(i32, i32)>,
) -> AppResult<Json<IncomeResponse>> {
    let owner_id = user.sub;

    let by_actor_rows = IncomeByActorRow::find_by_statement(Statement::from_sql_and_values(
        DbBackend::Postgres,
        r#"
        SELECT
            a.id   AS actor_id,
            a.name AS actor_name,
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
            ) AS total
        FROM ledger_actors a
        WHERE a.owner_id = $1
        ORDER BY a.name
        "#,
        [owner_id.into(), year.into(), month.into()],
    ))
    .all(&*db)
    .await?;

    let by_actor: Vec<IncomeByActor> = by_actor_rows
        .into_iter()
        .map(|r| IncomeByActor {
            actor_id: Some(r.actor_id),
            actor_name: r.actor_name,
            total: r.total,
        })
        .collect();

    let total: Decimal = by_actor.iter().map(|e| e.total).sum();

    // ── categories breakdown (income kind, sign preserved) ──────────────────
    let cat_rows = IncomeCatRow::find_by_statement(Statement::from_sql_and_values(
        DbBackend::Postgres,
        r#"
        SELECT
            c.id        AS category_id,
            c.name      AS category_name,
            c.kind      AS kind,
            a.id        AS actor_id,
            a.name      AS actor_name,
            (SUM(t.amount))::numeric(15,2) AS amount
        FROM transactions t
        JOIN categories c         ON c.id = t.category_id AND c.owner_id = t.owner_id
        LEFT JOIN ledger_actors a ON a.id = t.actor_id    AND a.owner_id = t.owner_id
        WHERE t.owner_id = $1
          AND c.kind = 'income'
          AND t.occurred_on >= make_date($2, $3, 1)
          AND t.occurred_on  < make_date($2, $3, 1) + INTERVAL '1 month'
        GROUP BY c.id, c.name, c.kind, a.id, a.name
        ORDER BY c.name, a.name
        "#,
        [owner_id.into(), year.into(), month.into()],
    ))
    .all(&*db)
    .await?;

    let mut category_order: Vec<Uuid> = Vec::new();
    let mut category_meta: HashMap<Uuid, (String, String)> = HashMap::new();
    let mut category_actors: HashMap<Uuid, Vec<ByActorEntry>> = HashMap::new();

    for row in cat_rows {
        let actor_id = row.actor_id;
        let actor_name = row.actor_name.unwrap_or_else(|| "(미지정)".to_string());

        if !category_meta.contains_key(&row.category_id) {
            category_order.push(row.category_id);
            category_meta.insert(row.category_id, (row.category_name.clone(), row.kind.clone()));
        }

        category_actors
            .entry(row.category_id)
            .or_default()
            .push(ByActorEntry {
                actor_id,
                actor_name,
                amount: row.amount,
            });
    }

    let categories: Vec<CategorySummary> = category_order
        .into_iter()
        .map(|cid| {
            let (name, kind) = category_meta.remove(&cid).unwrap();
            let by_actor = category_actors.remove(&cid).unwrap_or_default();
            let cat_total: Decimal = by_actor
                .iter()
                .map(|e| e.amount)
                .fold(Decimal::ZERO, |acc, x| acc + x);
            CategorySummary {
                category_id: cid,
                category_name: name,
                kind,
                by_actor,
                total: cat_total,
            }
        })
        .collect();

    Ok(Json(IncomeResponse {
        month: format!("{:04}-{:02}", year, month),
        by_actor,
        total,
        categories,
    }))
}
