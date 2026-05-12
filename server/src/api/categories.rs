use axum::{extract::{Path, State}, Json};
use sea_orm::{ActiveModelTrait, ActiveValue::Set, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

use crate::auth::ExtractUser;
use crate::entity::{categories, ledger_actors, merchants, payment_methods, prelude::*};
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
    let rows = Categories::find()
        .filter(categories::Column::OwnerId.eq(user.sub))
        .order_by_asc(categories::Column::Kind)
        .order_by_asc(categories::Column::Name)
        .all(&*db)
        .await?;

    Ok(Json(rows.into_iter().map(|r| CategoryItem {
        id: r.id,
        name: r.name,
        kind: r.kind,
        review_state: r.review_state,
        parent_id: r.parent_id,
    }).collect()))
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
    let rows = Merchants::find()
        .filter(merchants::Column::OwnerId.eq(user.sub))
        .order_by_asc(merchants::Column::Name)
        .all(&*db)
        .await?;

    Ok(Json(rows.into_iter().map(|r| MerchantItem {
        id: r.id,
        name: r.name,
        review_state: r.review_state,
    }).collect()))
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
    let rows: Vec<(payment_methods::Model, Option<ledger_actors::Model>)> =
        PaymentMethods::find()
            .find_also_related(LedgerActors)
            .filter(payment_methods::Column::OwnerId.eq(user.sub))
            .order_by_asc(payment_methods::Column::Name)
            .all(&*db)
            .await?;

    Ok(Json(rows.into_iter().map(|(pm, actor)| PaymentMethodItem {
        id: pm.id,
        name: pm.name,
        actor_id: pm.actor_id,
        actor_name: actor.map(|a| a.name),
        review_state: pm.review_state,
    }).collect()))
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
    if body.kind != "income" && body.kind != "expense" {
        return Err(AppError::BadRequest(
            "kind must be 'income' or 'expense'".into(),
        ));
    }

    let cat = Categories::find()
        .filter(categories::Column::Id.eq(category_id))
        .filter(categories::Column::OwnerId.eq(user.sub))
        .one(&*db)
        .await?
        .ok_or_else(|| AppError::NotFound("category not found".into()))?;

    if cat.name == "차감" {
        return Err(AppError::Conflict(json!({
            "error": "protected_category",
            "message": "차감 is a protected category and cannot be re-typed",
        })));
    }

    let mut active: categories::ActiveModel = cat.into();
    active.kind = Set(body.kind.clone());
    active.update(&*db).await?;

    Ok(Json(PatchCategoryKindResponse {
        id: category_id,
        kind: body.kind,
    }))
}

// ── PATCH /api/payment-methods/:id/actor ────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct PatchPaymentMethodActorBody {
    pub actor_id: Uuid,
}

#[derive(Debug, Serialize)]
pub struct PatchPaymentMethodActorResponse {
    pub id: Uuid,
    pub name: String,
    pub actor_id: Uuid,
    pub review_state: String,
}

pub async fn handle_patch_payment_method_actor(
    State(db): State<Arc<DatabaseConnection>>,
    ExtractUser(user): ExtractUser,
    Path(pm_id): Path<Uuid>,
    Json(body): Json<PatchPaymentMethodActorBody>,
) -> AppResult<Json<PatchPaymentMethodActorResponse>> {
    // Verify actor belongs to this owner
    LedgerActors::find()
        .filter(ledger_actors::Column::OwnerId.eq(user.sub))
        .filter(ledger_actors::Column::Id.eq(body.actor_id))
        .one(&*db)
        .await?
        .ok_or_else(|| AppError::BadRequest("actor not found or not owned by user".into()))?;

    // Find the payment method
    let pm = PaymentMethods::find()
        .filter(payment_methods::Column::OwnerId.eq(user.sub))
        .filter(payment_methods::Column::Id.eq(pm_id))
        .one(&*db)
        .await?
        .ok_or_else(|| AppError::NotFound("payment method not found".into()))?;

    let mut active: payment_methods::ActiveModel = pm.into();
    active.actor_id = Set(Some(body.actor_id));
    let updated = active.update(&*db).await?;

    Ok(Json(PatchPaymentMethodActorResponse {
        id: updated.id,
        name: updated.name,
        actor_id: body.actor_id,
        review_state: updated.review_state,
    }))
}
