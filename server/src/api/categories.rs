use axum::{extract::State, Json};
use serde::Serialize;
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;

use crate::auth::ExtractUser;
use crate::error::AppResult;

// ── GET /api/categories ──────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct CategoryItem {
    pub id: Uuid,
    pub name: String,
    pub kind: String,
    pub review_state: String,
    pub parent_id: Option<Uuid>,
}

pub async fn handle_get_categories(
    State(pool): State<Arc<PgPool>>,
    ExtractUser(user): ExtractUser,
) -> AppResult<Json<Vec<CategoryItem>>> {
    let owner_id = user.sub;

    let rows = sqlx::query!(
        r#"
        SELECT
            id       AS "id!: Uuid",
            name     AS "name!: String",
            kind     AS "kind!: String",
            review_state AS "review_state!: String",
            parent_id AS "parent_id?: Uuid"
        FROM categories
        WHERE owner_id = $1
        ORDER BY kind, name
        "#,
        owner_id
    )
    .fetch_all(&*pool)
    .await?;

    let items = rows
        .into_iter()
        .map(|r| CategoryItem {
            id: r.id,
            name: r.name,
            kind: r.kind,
            review_state: r.review_state,
            parent_id: r.parent_id,
        })
        .collect();

    Ok(Json(items))
}

// ── GET /api/merchants ───────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct MerchantItem {
    pub id: Uuid,
    pub name: String,
    pub review_state: String,
}

pub async fn handle_get_merchants(
    State(pool): State<Arc<PgPool>>,
    ExtractUser(user): ExtractUser,
) -> AppResult<Json<Vec<MerchantItem>>> {
    let owner_id = user.sub;

    let rows = sqlx::query!(
        r#"
        SELECT
            id           AS "id!: Uuid",
            name         AS "name!: String",
            review_state AS "review_state!: String"
        FROM merchants
        WHERE owner_id = $1
        ORDER BY name
        "#,
        owner_id
    )
    .fetch_all(&*pool)
    .await?;

    let items = rows
        .into_iter()
        .map(|r| MerchantItem {
            id: r.id,
            name: r.name,
            review_state: r.review_state,
        })
        .collect();

    Ok(Json(items))
}

// ── GET /api/payment-methods ─────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct PaymentMethodItem {
    pub id: Uuid,
    pub name: String,
    pub actor_id: Option<Uuid>,
    pub actor_name: Option<String>,
    pub review_state: String,
}

pub async fn handle_get_payment_methods(
    State(pool): State<Arc<PgPool>>,
    ExtractUser(user): ExtractUser,
) -> AppResult<Json<Vec<PaymentMethodItem>>> {
    let owner_id = user.sub;

    let rows = sqlx::query!(
        r#"
        SELECT
            pm.id           AS "id!: Uuid",
            pm.name         AS "name!: String",
            pm.actor_id     AS "actor_id?: Uuid",
            la.name         AS "actor_name?: String",
            pm.review_state AS "review_state!: String"
        FROM payment_methods pm
        LEFT JOIN ledger_actors la ON la.id = pm.actor_id AND la.owner_id = pm.owner_id
        WHERE pm.owner_id = $1
        ORDER BY pm.name
        "#,
        owner_id
    )
    .fetch_all(&*pool)
    .await?;

    let items = rows
        .into_iter()
        .map(|r| PaymentMethodItem {
            id: r.id,
            name: r.name,
            actor_id: r.actor_id,
            actor_name: r.actor_name,
            review_state: r.review_state,
        })
        .collect();

    Ok(Json(items))
}

