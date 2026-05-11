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

use crate::auth::ExtractUser;
use crate::error::AppResult;

#[derive(Debug, Serialize)]
pub struct ActorRef {
    pub actor_id: Option<Uuid>,
    pub actor_name: String,
}

#[derive(Debug, Serialize)]
pub struct ByActorEntry {
    pub actor_id: Option<Uuid>,
    pub actor_name: String,
    /// (category, actor) 셀의 합계. 호출 컨텍스트별 부호 규약:
    /// - expense summary (`/api/summary`): `-SUM(t.amount)` — 지출은 저장상 음수라 부호 뒤집어 양수화.
    ///   음수가 나오면 환불이 일반 지출보다 컸다는 뜻. 프론트는 `Math.abs()` 로 슬라이스 크기 사용.
    /// - income summary (`/api/summary/income`): `SUM(t.amount)` 그대로 — 수입은 저장상 양수.
    pub amount: Decimal,
}

#[derive(Debug, Serialize)]
pub struct CategorySummary {
    pub category_id: Uuid,
    pub category_name: String,
    pub kind: String,
    /// 액터별 분해. 카테고리 합계 부호와 동일한 의미 체계.
    pub by_actor: Vec<ByActorEntry>,
    /// 액터 합계의 단순 합. 부호 규약은 `ByActorEntry::amount` 와 동일 — 호출 엔드포인트에 따라 다름.
    pub total: Decimal,
}

#[derive(Debug, Serialize)]
pub struct SummaryResponse {
    pub year: i32,
    pub month: i32,
    pub categories: Vec<CategorySummary>,
    pub actors: Vec<ActorRef>,
}

#[derive(FromQueryResult)]
struct SummaryRow {
    category_id: Uuid,
    category_name: String,
    kind: String,
    actor_id: Option<Uuid>,
    actor_name: Option<String>,
    amount: Decimal,
}

/// GET /api/summary/:year/:month
///
/// 지출 카테고리(`kind='expense'`) 만 반환한다. 수입은 별도 엔드포인트(`/api/summary/income`).
/// amount = -SUM(t.amount) 로 양수화 (저장상 지출은 음수).
pub async fn handle_get_summary(
    State(db): State<Arc<DatabaseConnection>>,
    ExtractUser(user): ExtractUser,
    Path((year, month)): Path<(i32, i32)>,
) -> AppResult<Json<SummaryResponse>> {
    let owner_id = user.sub;

    let rows = SummaryRow::find_by_statement(Statement::from_sql_and_values(
        DbBackend::Postgres,
        r#"
        SELECT
            c.id        AS category_id,
            c.name      AS category_name,
            c.kind      AS kind,
            a.id        AS actor_id,
            a.name      AS actor_name,
            (-SUM(t.amount))::numeric(15,2) AS amount
        FROM transactions t
        JOIN categories c         ON c.id = t.category_id AND c.owner_id = t.owner_id
        LEFT JOIN ledger_actors a ON a.id = t.actor_id    AND a.owner_id = t.owner_id
        WHERE t.owner_id = $1
          AND c.kind = 'expense'
          AND t.occurred_on >= make_date($2, $3, 1)
          AND t.occurred_on  < make_date($2, $3, 1) + INTERVAL '1 month'
        GROUP BY c.id, c.name, c.kind, a.id, a.name
        ORDER BY c.name, a.name
        "#,
        [owner_id.into(), year.into(), month.into()],
    ))
    .all(&*db)
    .await?;

    let mut actor_order: Vec<Option<Uuid>> = Vec::new();
    let mut actor_map: HashMap<Option<Uuid>, String> = HashMap::new();

    let mut category_order: Vec<Uuid> = Vec::new();
    let mut category_meta: HashMap<Uuid, (String, String)> = HashMap::new();
    let mut category_actors: HashMap<Uuid, Vec<ByActorEntry>> = HashMap::new();

    for row in rows {
        let actor_id = row.actor_id;
        let actor_name = row.actor_name.unwrap_or_else(|| "(미지정)".to_string());

        if !actor_map.contains_key(&actor_id) {
            actor_order.push(actor_id);
            actor_map.insert(actor_id, actor_name.clone());
        }

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
            let total: Decimal = by_actor
                .iter()
                .map(|e| e.amount)
                .fold(Decimal::ZERO, |acc, x| acc + x);
            CategorySummary {
                category_id: cid,
                category_name: name,
                kind,
                by_actor,
                total,
            }
        })
        .collect();

    let actors: Vec<ActorRef> = actor_order
        .into_iter()
        .map(|aid| ActorRef {
            actor_id: aid,
            actor_name: actor_map.remove(&aid).unwrap(),
        })
        .collect();

    Ok(Json(SummaryResponse {
        year,
        month,
        categories,
        actors,
    }))
}
