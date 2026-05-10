use axum::{extract::{Path, State}, Json};
use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

use crate::auth::ExtractUser;
use crate::error::{AppError, AppResult};

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
    State(db): State<Arc<DatabaseConnection>>,
    ExtractUser(user): ExtractUser,
) -> AppResult<Json<Vec<CategoryItem>>> {
    let pool = crate::db::pool_of(&db);
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
    .fetch_all(pool)
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
    State(db): State<Arc<DatabaseConnection>>,
    ExtractUser(user): ExtractUser,
) -> AppResult<Json<Vec<MerchantItem>>> {
    let pool = crate::db::pool_of(&db);
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
    .fetch_all(pool)
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
    State(db): State<Arc<DatabaseConnection>>,
    ExtractUser(user): ExtractUser,
) -> AppResult<Json<Vec<PaymentMethodItem>>> {
    let pool = crate::db::pool_of(&db);
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
    .fetch_all(pool)
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

// ── PATCH /api/categories/:id/kind ───────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct PatchCategoryKindBody {
    pub kind: String,
}

#[derive(Debug, Serialize)]
pub struct PatchCategoryKindResponse {
    pub id: Uuid,
    pub kind: String,
}

/// PATCH /api/categories/:id/kind — toggle income/expense classification.
/// `차감` 카테고리는 시스템 보호 카테고리이므로 변경 불가.
pub async fn handle_patch_category_kind(
    State(db): State<Arc<DatabaseConnection>>,
    ExtractUser(user): ExtractUser,
    Path(category_id): Path<Uuid>,
    Json(body): Json<PatchCategoryKindBody>,
) -> AppResult<Json<PatchCategoryKindResponse>> {
    let pool = crate::db::pool_of(&db);
    let owner_id = user.sub;

    if body.kind != "income" && body.kind != "expense" {
        return Err(AppError::BadRequest(
            "kind must be 'income' or 'expense'".into(),
        ));
    }

    let row = sqlx::query!(
        r#"SELECT name AS "name!: String" FROM categories WHERE id = $1 AND owner_id = $2"#,
        category_id,
        owner_id
    )
    .fetch_optional(pool)
    .await?;

    let Some(found) = row else {
        return Err(AppError::NotFound("category not found".into()));
    };
    if found.name == "차감" {
        return Err(AppError::Conflict(json!({
            "error": "protected_category",
            "message": "차감 is a protected category and cannot be re-typed",
        })));
    }

    sqlx::query!(
        r#"UPDATE categories SET kind = $1 WHERE id = $2 AND owner_id = $3"#,
        body.kind,
        category_id,
        owner_id
    )
    .execute(pool)
    .await?;

    Ok(Json(PatchCategoryKindResponse {
        id: category_id,
        kind: body.kind,
    }))
}

