use axum::{
    extract::{Path, State},
    Json,
};
use rust_decimal::Decimal;
use serde::Serialize;
use sqlx::PgPool;
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

use crate::auth::ExtractUser;
use crate::error::AppResult;

// ── Response types ────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct ActorRef {
    /// NULL when the transaction has no actor_id or the actor row is missing.
    pub actor_id: Option<Uuid>,
    pub actor_name: String,
}

#[derive(Debug, Serialize)]
pub struct ByActorEntry {
    /// NULL when the transaction has no actor_id or the actor row is missing.
    pub actor_id: Option<Uuid>,
    pub actor_name: String,
    /// Absolute value of the net signed sum for this (category, actor) cell.
    pub amount: Decimal,
    /// Direction: +1 = net expense, -1 = net income/refund, 0 = zero.
    pub sign: i16,
}

#[derive(Debug, Serialize)]
pub struct CategorySummary {
    pub category_id: Uuid,
    pub category_name: String,
    pub kind: String,
    /// Per-actor breakdown for this category.
    pub by_actor: Vec<ByActorEntry>,
    /// Absolute net total across all actors: abs(SUM(amount * sign)).
    /// Reflects the same directionality as the by_actor entries.
    pub total: Decimal,
}

#[derive(Debug, Serialize)]
pub struct SummaryResponse {
    pub year: i32,
    pub month: i32,
    pub categories: Vec<CategorySummary>,
    pub actors: Vec<ActorRef>,
}

// ── Handler ───────────────────────────────────────────────────────────────────

/// GET /api/summary/:year/:month
///
/// Category × actor pivot for the requested month.
/// Amount semantics: SUM(amount * sign) per (category, actor).
/// "차감" is included as a normal category row here; /api/settlement separates it.
pub async fn handle_get_summary(
    State(pool): State<Arc<PgPool>>,
    ExtractUser(user): ExtractUser,
    Path((year, month)): Path<(i32, i32)>,
) -> AppResult<Json<SummaryResponse>> {
    let owner_id = user.sub;

    // SUM(amount * sign) gives the net signed total per (category, actor).
    // LEFT JOIN ledger_actors so that transactions with NULL actor_id are included
    // and grouped under a synthetic "(미지정)" actor.
    let rows = sqlx::query!(
        r#"
        SELECT
            c.id        AS "category_id!: Uuid",
            c.name      AS "category_name!: String",
            c.kind      AS "kind!: String",
            a.id        AS "actor_id?: Uuid",
            a.name      AS "actor_name?: String",
            SUM(t.amount * t.sign)::numeric(15,2) AS "net_amount!: Decimal"
        FROM transactions t
        JOIN categories c         ON c.id = t.category_id AND c.owner_id = t.owner_id
        LEFT JOIN ledger_actors a ON a.id = t.actor_id    AND a.owner_id = t.owner_id
        WHERE t.owner_id = $1
          AND t.occurred_on >= make_date($2, $3, 1)
          AND t.occurred_on  < make_date($2, $3, 1) + INTERVAL '1 month'
        GROUP BY c.id, c.name, c.kind, a.id, a.name
        ORDER BY c.kind, c.name, a.name
        "#,
        owner_id,
        year,
        month,
    )
    .fetch_all(&*pool)
    .await?;

    // Collect unique actors (in order of first appearance).
    // actor_id=None maps to the synthetic "(미지정)" slot.
    let mut actor_order: Vec<Option<Uuid>> = Vec::new();
    let mut actor_map: HashMap<Option<Uuid>, String> = HashMap::new();

    // Group rows by category.
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

        // Derive sign from the net: positive net → sign=1, negative → sign=-1, zero → 0.
        let net = row.net_amount;
        let sign: i16 = if net > Decimal::ZERO {
            1
        } else if net < Decimal::ZERO {
            -1
        } else {
            0
        };

        category_actors
            .entry(row.category_id)
            .or_default()
            .push(ByActorEntry {
                actor_id,
                actor_name,
                amount: net.abs(),
                sign,
            });
    }

    // Build response.
    let categories: Vec<CategorySummary> = category_order
        .into_iter()
        .map(|cid| {
            let (name, kind) = category_meta.remove(&cid).unwrap();
            let by_actor = category_actors.remove(&cid).unwrap_or_default();
            // Category total = sum of (amount * sign) across actors.
            let net_total: Decimal = by_actor
                .iter()
                .map(|e| e.amount * Decimal::from(e.sign))
                .fold(Decimal::ZERO, |acc, x| acc + x);
            CategorySummary {
                category_id: cid,
                category_name: name,
                kind,
                by_actor,
                total: net_total.abs(),
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
