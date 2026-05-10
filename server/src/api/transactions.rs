use axum::{
    extract::{Query, State},
    Json,
};
use chrono::NaiveDate;
use rust_decimal::Decimal;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, FromQueryResult, Statement};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

use crate::auth::ExtractUser;
use crate::error::AppResult;

#[derive(Debug, Deserialize, Default)]
pub struct TransactionQuery {
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
    pub category: Option<String>,
    pub actor: Option<String>,
    pub merchant: Option<String>,
    pub payment: Option<String>,
    pub product: Option<String>,
    pub group: Option<Uuid>,
}

#[derive(Debug, Serialize, Clone)]
pub struct TransactionItem {
    pub id: Uuid,
    pub group_id: Uuid,
    pub occurred_on: NaiveDate,
    pub merchant_id: Option<Uuid>,
    pub merchant_name: Option<String>,
    pub actor_id: Option<Uuid>,
    pub actor_name: Option<String>,
    pub category_id: Option<Uuid>,
    pub category_name: Option<String>,
    pub product_id: Option<Uuid>,
    pub product_name: Option<String>,
    pub payment_method_id: Option<Uuid>,
    pub payment_method_name: Option<String>,
    pub amount: Decimal,
    pub unit_price: Option<Decimal>,
    pub quantity: Option<Decimal>,
    pub memo: Option<String>,
    /// multi-line 그룹에서 같은 group_id를 가진 나머지 행들
    pub children: Vec<TransactionItem>,
}

#[derive(Debug, Serialize)]
pub struct TransactionsResponse {
    pub items: Vec<TransactionItem>,
    pub total: usize,
}

/// Flat row fetched from the DB — matched to TransactionItem fields (children excluded).
#[derive(Debug, FromQueryResult)]
struct TxRow {
    id: Uuid,
    group_id: Uuid,
    occurred_on: NaiveDate,
    merchant_id: Option<Uuid>,
    merchant_name: Option<String>,
    actor_id: Option<Uuid>,
    actor_name: Option<String>,
    category_id: Option<Uuid>,
    category_name: Option<String>,
    product_id: Option<Uuid>,
    product_name: Option<String>,
    payment_method_id: Option<Uuid>,
    payment_method_name: Option<String>,
    amount: Decimal,
    unit_price: Option<Decimal>,
    quantity: Option<Decimal>,
    memo: Option<String>,
}

/// GET /api/transactions
/// 필터: from/to/category/actor/merchant/payment/product/group
/// multi-line 그룹은 group_id로 묶어 children 배열로 반환
///
/// Uses raw SQL via Statement because the query joins 5 tables and applies
/// nullable optional filters (`$N::type IS NULL OR col = $N`) — a pattern
/// that does not compose cleanly through SeaORM's SelectModel.
pub async fn handle_get_transactions(
    State(db): State<Arc<DatabaseConnection>>,
    ExtractUser(user): ExtractUser,
    Query(q): Query<TransactionQuery>,
) -> AppResult<Json<TransactionsResponse>> {
    let owner_id = user.sub;

    let stmt = Statement::from_sql_and_values(
        DbBackend::Postgres,
        r#"
        SELECT
            t.id           AS id,
            t.group_id     AS group_id,
            t.occurred_on  AS occurred_on,
            t.merchant_id  AS merchant_id,
            m.name         AS merchant_name,
            t.actor_id     AS actor_id,
            a.name         AS actor_name,
            t.category_id  AS category_id,
            c.name         AS category_name,
            t.product_id   AS product_id,
            p.name         AS product_name,
            t.payment_method_id AS payment_method_id,
            pm.name        AS payment_method_name,
            t.amount       AS amount,
            t.unit_price   AS unit_price,
            t.quantity     AS quantity,
            t.memo         AS memo
        FROM transactions t
        LEFT JOIN merchants m     ON m.id  = t.merchant_id     AND m.owner_id  = t.owner_id
        LEFT JOIN ledger_actors a ON a.id  = t.actor_id        AND a.owner_id  = t.owner_id
        LEFT JOIN categories c    ON c.id  = t.category_id     AND c.owner_id  = t.owner_id
        LEFT JOIN products p      ON p.id  = t.product_id      AND p.owner_id  = t.owner_id
        LEFT JOIN payment_methods pm ON pm.id = t.payment_method_id AND pm.owner_id = t.owner_id
        WHERE t.owner_id = $1
          AND ($2::date IS NULL OR t.occurred_on >= $2)
          AND ($3::date IS NULL OR t.occurred_on <= $3)
          AND ($4::text IS NULL OR c.name = $4)
          AND ($5::text IS NULL OR a.name = $5)
          AND ($6::text IS NULL OR m.name = $6)
          AND ($7::text IS NULL OR pm.name = $7)
          AND ($8::text IS NULL OR p.name = $8)
          AND ($9::uuid IS NULL OR t.group_id = $9)
        ORDER BY t.occurred_on DESC, t.id
        "#,
        [
            owner_id.into(),
            q.from.into(),
            q.to.into(),
            q.category.into(),
            q.actor.into(),
            q.merchant.into(),
            q.payment.into(),
            q.product.into(),
            q.group.into(),
        ],
    );

    let rows = TxRow::find_by_statement(stmt).all(&*db).await?;

    // group_id별로 묶기
    let mut group_map: HashMap<Uuid, Vec<TransactionItem>> = HashMap::new();
    let mut group_order: Vec<Uuid> = Vec::new();

    for row in rows {
        let item = TransactionItem {
            id: row.id,
            group_id: row.group_id,
            occurred_on: row.occurred_on,
            merchant_id: row.merchant_id,
            merchant_name: row.merchant_name,
            actor_id: row.actor_id,
            actor_name: row.actor_name,
            category_id: row.category_id,
            category_name: row.category_name,
            product_id: row.product_id,
            product_name: row.product_name,
            payment_method_id: row.payment_method_id,
            payment_method_name: row.payment_method_name,
            amount: row.amount,
            unit_price: row.unit_price,
            quantity: row.quantity,
            memo: row.memo,
            children: Vec::new(),
        };

        let entry = group_map.entry(row.group_id).or_insert_with(|| {
            group_order.push(row.group_id);
            Vec::new()
        });
        entry.push(item);
    }

    // 결과 조립: multi-line 그룹은 첫 번째 행이 대표, 나머지는 children
    let mut items: Vec<TransactionItem> = Vec::new();
    for gid in group_order {
        let group_rows = group_map.remove(&gid).unwrap_or_default();
        if group_rows.len() == 1 {
            items.push(group_rows.into_iter().next().unwrap());
        } else {
            let mut iter = group_rows.into_iter();
            let mut first = iter.next().unwrap();
            first.children = iter.collect();
            items.push(first);
        }
    }

    let total = items.len();
    Ok(Json(TransactionsResponse { items, total }))
}
