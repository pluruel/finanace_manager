/// M2 Step B — Alias CRUD + review queue + auto-remap
///
/// Endpoints implemented here:
///   GET  /api/review-queue?scope=category|merchant|payment_method|actor|product
///   POST /api/aliases            body { scope, raw_text, target_id }
///   DELETE /api/aliases/:id
///   POST /api/entities/:scope/:id/confirm

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait,
    DatabaseBackend, DatabaseConnection, DatabaseTransaction, EntityTrait,
    QueryFilter, QueryOrder, QuerySelect, Statement, TransactionTrait,
};
use sea_orm::sea_query::LockType;
use sea_orm::FromQueryResult;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use uuid::Uuid;

use crate::auth::ExtractUser;
use crate::entity::{
    aliases, categories, ledger_actors, merchants, payment_methods, products,
    prelude::*,
};
use crate::error::{AppError, AppResult};
use crate::import::normalize::to_norm_key;

// ── Levenshtein edit distance (simple iterative, Rust) ───────────────────────

/// Returns the Levenshtein distance between two strings (character-level).
/// Optimised for small strings (entity names are well under 100 chars).
fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let na = a.len();
    let nb = b.len();

    if na == 0 {
        return nb;
    }
    if nb == 0 {
        return na;
    }

    // Single-row DP (space-efficient).
    let mut prev: Vec<usize> = (0..=nb).collect();
    let mut curr = vec![0usize; nb + 1];

    for i in 1..=na {
        curr[0] = i;
        for j in 1..=nb {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            curr[j] = (prev[j] + 1)
                .min(curr[j - 1] + 1)
                .min(prev[j - 1] + cost);
        }
        prev.clone_from(&curr);
    }
    prev[nb]
}

// ── Scope helpers ─────────────────────────────────────────────────────────────

/// Maps scope string → (entity table, id column, name column, transactions FK column).
///
/// Returns None for unknown scopes.
fn scope_meta(scope: &str) -> Option<(&'static str, &'static str)> {
    match scope {
        "category" => Some(("categories", "category_id")),
        "merchant" => Some(("merchants", "merchant_id")),
        "payment_method" => Some(("payment_methods", "payment_method_id")),
        "actor" => Some(("ledger_actors", "actor_id")),
        "product" => Some(("products", "product_id")),
        _ => None,
    }
}

const ALL_SCOPES: &[&str] = &[
    "category",
    "merchant",
    "payment_method",
    "actor",
    "product",
];

// ── GET /api/review-queue ─────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct ReviewQueueQuery {
    scope: Option<String>,
}

/// A single alias row that maps raw_text → entity.
#[derive(Debug, Serialize)]
pub struct AliasInfo {
    pub alias_id: Uuid,
    pub raw_text: String,
    pub norm_key: String,
}

/// One entry in the review-queue response.
#[derive(Debug, Serialize)]
pub struct ReviewQueueItem {
    pub scope: String,
    pub id: Uuid,
    pub name: String,
    pub review_state: String,
    /// Present only for `scope = "category"`. Null for all other scopes.
    pub kind: Option<String>,
    pub raw_texts: Vec<AliasInfo>,
    pub merge_candidates: Vec<MergeCandidate>,
}

#[derive(Debug, Serialize)]
pub struct MergeCandidate {
    pub id: Uuid,
    pub name: String,
}

pub async fn handle_get_review_queue(
    State(db): State<Arc<DatabaseConnection>>,
    ExtractUser(user): ExtractUser,
    Query(params): Query<ReviewQueueQuery>,
) -> AppResult<Json<Vec<ReviewQueueItem>>> {
    let owner_id = user.sub;

    let scopes_to_query: Vec<&str> = match &params.scope {
        Some(s) => {
            if scope_meta(s).is_none() {
                return Err(AppError::BadRequest(format!(
                    "Unknown scope '{}'. Valid: category, merchant, payment_method, actor, product",
                    s
                )));
            }
            vec![s.as_str()]
        }
        None => ALL_SCOPES.to_vec(),
    };

    let mut result: Vec<ReviewQueueItem> = Vec::new();

    for scope in scopes_to_query {
        let items = review_queue_for_scope(&*db, owner_id, scope).await?;
        result.extend(items);
    }

    Ok(Json(result))
}

/// Fetch pending entities for a given scope and build review-queue items with
/// their aliases and merge candidates.
///
/// Builds three indexes up front so the per-pending work is bounded by
/// candidate count instead of full table scans:
///   - `entity_aliases`:   entity_id → its AliasInfo list (also gives raw_texts directly)
///   - `norm_to_entities`: norm_key  → entity_ids sharing it (shared-norm-key candidates in O(1))
///   - `by_len`:           name char-length → indexes into `lowered` (Levenshtein candidates
///                         pruned to entities within ±1 char of the target name)
async fn review_queue_for_scope(
    db: &DatabaseConnection,
    owner_id: Uuid,
    scope: &str,
) -> Result<Vec<ReviewQueueItem>, AppError> {
    use std::collections::{HashMap, HashSet};

    // Fetch all entities of this scope for this owner (needed for merge candidates too).
    let all_entities = fetch_all_entities(db, owner_id, scope).await?;

    // Fetch all aliases for this scope+owner.
    let all_aliases_models = Aliases::find()
        .filter(aliases::Column::OwnerId.eq(owner_id))
        .filter(aliases::Column::Scope.eq(scope))
        .all(db)
        .await?;

    // ── Index 1+2: per-entity aliases and norm_key reverse index ─────────────
    let mut entity_aliases: HashMap<Uuid, Vec<AliasInfo>> =
        HashMap::with_capacity(all_entities.len());
    let mut norm_to_entities: HashMap<String, Vec<Uuid>> = HashMap::new();
    for r in all_aliases_models {
        norm_to_entities
            .entry(r.norm_key.clone())
            .or_default()
            .push(r.target_id);
        entity_aliases
            .entry(r.target_id)
            .or_default()
            .push(AliasInfo {
                alias_id: r.id,
                raw_text: r.raw_text,
                norm_key: r.norm_key,
            });
    }
    // The same (norm_key, entity) pair can appear via multiple alias rows; dedup so
    // shared-norm-key lookup doesn't yield duplicate candidate ids.
    for v in norm_to_entities.values_mut() {
        v.sort_unstable();
        v.dedup();
    }

    // ── Pre-lowercased names + length bucket for Levenshtein pruning ─────────
    // Carries (id, name, name_lower, kind, review_state) so the outer iteration
    // stays in fetch_all_entities order (name-sorted) without re-scanning.
    let lowered: Vec<(Uuid, String, String, Option<String>, String)> = all_entities
        .into_iter()
        .map(|(id, name, rs, kind)| {
            let lower = name.to_lowercase();
            (id, name, lower, kind, rs)
        })
        .collect();

    let mut by_len: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut name_lens: Vec<usize> = Vec::with_capacity(lowered.len());
    for (i, t) in lowered.iter().enumerate() {
        let len = t.2.chars().count();
        name_lens.push(len);
        by_len.entry(len).or_default().push(i);
    }

    // ── Build pending items ──────────────────────────────────────────────────
    let mut items = Vec::new();
    for (idx, (eid, ename, ename_lower, ekind, rs)) in lowered.iter().enumerate() {
        if rs != "pending" {
            continue;
        }

        let raw_texts = entity_aliases.remove(eid).unwrap_or_default();

        // Candidate ids via two paths, deduped through a HashSet.
        let mut cand_ids: HashSet<Uuid> = HashSet::new();

        // (a) Shared norm_key — O(aliases of this entity × bucket size).
        for a in &raw_texts {
            if let Some(others) = norm_to_entities.get(&a.norm_key) {
                for oid in others {
                    if oid != eid {
                        cand_ids.insert(*oid);
                    }
                }
            }
        }

        // (b) Levenshtein ≤ 1 on canonical names — only consider names within
        //     ±1 char length, since |edit_distance| ≥ |len_a - len_b|.
        let len_e = name_lens[idx];
        let lens_to_check: [usize; 3] = [len_e.saturating_sub(1), len_e, len_e + 1];
        for d in lens_to_check {
            if let Some(idxs) = by_len.get(&d) {
                for &i in idxs {
                    let (oid, _, lower_o, _, _) = &lowered[i];
                    if oid == eid || cand_ids.contains(oid) {
                        continue;
                    }
                    if levenshtein(ename_lower, lower_o) <= 1 {
                        cand_ids.insert(*oid);
                    }
                }
            }
        }

        // Materialize merge_candidates in fetch_all_entities order (name-sorted) for
        // deterministic API responses.
        let merge_candidates: Vec<MergeCandidate> = lowered
            .iter()
            .filter(|(id, _, _, _, _)| cand_ids.contains(id))
            .map(|(id, name, _, _, _)| MergeCandidate {
                id: *id,
                name: name.clone(),
            })
            .collect();

        items.push(ReviewQueueItem {
            scope: scope.to_string(),
            id: *eid,
            name: ename.clone(),
            review_state: "pending".to_string(),
            kind: ekind.clone(),
            raw_texts,
            merge_candidates,
        });
    }

    Ok(items)
}

/// Returns (id, name, review_state, kind) for all entities of the given scope.
/// `kind` is Some("income"|"expense") for category scope; None for all other scopes.
async fn fetch_all_entities(
    db: &DatabaseConnection,
    owner_id: Uuid,
    scope: &str,
) -> Result<Vec<(Uuid, String, String, Option<String>)>, AppError> {
    let rows = match scope {
        "category" => Categories::find()
            .filter(categories::Column::OwnerId.eq(owner_id))
            .order_by_asc(categories::Column::Name)
            .all(db)
            .await?
            .into_iter()
            .map(|r| (r.id, r.name, r.review_state, Some(r.kind)))
            .collect(),

        "merchant" => Merchants::find()
            .filter(merchants::Column::OwnerId.eq(owner_id))
            .order_by_asc(merchants::Column::Name)
            .all(db)
            .await?
            .into_iter()
            .map(|r| (r.id, r.name, r.review_state, None))
            .collect(),

        "payment_method" => PaymentMethods::find()
            .filter(payment_methods::Column::OwnerId.eq(owner_id))
            .order_by_asc(payment_methods::Column::Name)
            .all(db)
            .await?
            .into_iter()
            .map(|r| (r.id, r.name, r.review_state, None))
            .collect(),

        "actor" => {
            // ledger_actors has no review_state. The 3 fixed actors (공동/엉아/아기)
            // do not need review.
            vec![]
        }

        "product" => Products::find()
            .filter(products::Column::OwnerId.eq(owner_id))
            .order_by_asc(products::Column::Name)
            .all(db)
            .await?
            .into_iter()
            .map(|r| (r.id, r.name, r.review_state, None))
            .collect(),

        _ => return Err(AppError::BadRequest(format!("Unknown scope: {}", scope))),
    };

    Ok(rows)
}

// ── POST /api/aliases ─────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct PostAliasBody {
    pub scope: String,
    pub raw_text: String,
    pub target_id: Uuid,
    /// Entity the client expected the alias to currently point to. When set,
    /// the merge path verifies the alias's current target_id matches this
    /// value under FOR UPDATE; a mismatch means another transaction already
    /// remapped the alias → 409 alias_changed. Optional for backward compat.
    #[serde(default)]
    pub source_id: Option<Uuid>,
}

#[derive(Serialize)]
pub struct PostAliasResponse {
    pub created: bool,
    pub remapped_transaction_count: i64,
    pub orphan_deleted: bool,
}

pub async fn handle_post_alias(
    State(db): State<Arc<DatabaseConnection>>,
    ExtractUser(user): ExtractUser,
    Json(body): Json<PostAliasBody>,
) -> AppResult<Json<PostAliasResponse>> {
    let owner_id = user.sub;
    let scope = &body.scope;

    if scope_meta(scope).is_none() {
        return Err(AppError::BadRequest(format!(
            "Unknown scope '{}'. Valid: category, merchant, payment_method, actor, product",
            scope
        )));
    }

    let norm = to_norm_key(&body.raw_text);

    let txn = db.begin().await?;

    // Acquire the alias row lock as the first SQL operation. Under READ COMMITTED,
    // a SELECT ... FOR UPDATE that races a concurrent UPDATE+COMMIT will block
    // until the other transaction commits, then re-read the row at the latest
    // committed version. This serialises concurrent merges of the same alias.
    // (Pattern E — lock_exclusive)
    let existing = Aliases::find()
        .filter(aliases::Column::OwnerId.eq(owner_id))
        .filter(aliases::Column::Scope.eq(scope.as_str()))
        .filter(aliases::Column::NormKey.eq(&norm))
        .lock(LockType::Update)
        .one(&txn)
        .await?;

    // If the client told us which entity it expected the alias to point to, verify
    // it under the lock. A mismatch means another transaction already remapped the
    // alias (in either fully-concurrent or sequential timing). This makes merge
    // races deterministic regardless of scheduler order.
    if let (Some(expected), Some(ref row)) = (body.source_id, existing.as_ref()) {
        if row.target_id != expected {
            return Err(AppError::Conflict(serde_json::json!({
                "error": "alias_changed",
                "message": "Another operation already remapped this alias.",
                "target_id": row.target_id,
            })));
        }
    }

    // Verify target entity exists for this owner+scope.
    verify_entity_exists(&txn, owner_id, scope, body.target_id).await?;

    // Check for 차감 protection (category scope).
    if scope == "category" {
        check_chagang_protection(&txn, owner_id, body.target_id, "target").await?;
    }

    let resp = match existing {
        None => {
            // Create path: no existing alias (or deleted between phases — treated as new).
            aliases::ActiveModel {
                owner_id: Set(owner_id),
                scope: Set(scope.clone()),
                raw_text: Set(body.raw_text.clone()),
                norm_key: Set(norm),
                target_id: Set(body.target_id),
                ..Default::default()
            }
            .insert(&txn)
            .await?;

            PostAliasResponse {
                created: true,
                remapped_transaction_count: 0,
                orphan_deleted: false,
            }
        }

        Some(ref existing_row) if existing_row.target_id == body.target_id => {
            // No-op path: alias already points to requested target.
            PostAliasResponse {
                created: false,
                remapped_transaction_count: 0,
                orphan_deleted: false,
            }
        }

        Some(existing_row) => {
            // Merge path: alias exists but points to a different target.
            let old_target_id = existing_row.target_id;
            let alias_id = existing_row.id;

            // 차감 protection for source (category scope).
            if scope == "category" {
                check_chagang_protection(&txn, owner_id, old_target_id, "source").await?;
            }

            // payment_method cross-actor check.
            if scope == "payment_method" {
                check_payment_method_actor_compatibility(
                    &txn,
                    owner_id,
                    old_target_id,
                    body.target_id,
                )
                .await?;
            }

            // SELECT ... FOR UPDATE on the source entity row to prevent concurrent
            // deletes or other entity-level races on the source entity.
            lock_entity_row(&txn, owner_id, scope, old_target_id).await?;

            // Update alias to point to new target.
            aliases::ActiveModel {
                id: Set(alias_id),
                target_id: Set(body.target_id),
                raw_text: Set(body.raw_text.clone()),
                ..Default::default()
            }
            .update(&txn)
            .await?;

            // Remap transactions.
            let remapped = remap_transactions(
                &txn,
                owner_id,
                scope,
                old_target_id,
                body.target_id,
            )
            .await?;

            // products.merchant_id has a non-cascading FK to merchants.id, so a
            // merchant merge must re-parent (or merge-by-name) its products before
            // the source merchant can be deleted.
            if scope == "merchant" {
                cascade_merge_merchant_products(
                    &txn,
                    owner_id,
                    vec![old_target_id],
                    body.target_id,
                )
                .await?;
            }

            // Optionally delete the orphaned source entity.
            let orphan_deleted =
                maybe_delete_orphan(&txn, owner_id, scope, old_target_id).await?;

            PostAliasResponse {
                created: false,
                remapped_transaction_count: remapped,
                orphan_deleted,
            }
        }
    };

    txn.commit().await?;
    Ok(Json(resp))
}

// ── DELETE /api/aliases/:id ───────────────────────────────────────────────────

pub async fn handle_delete_alias(
    State(db): State<Arc<DatabaseConnection>>,
    ExtractUser(user): ExtractUser,
    Path(alias_id): Path<Uuid>,
) -> AppResult<StatusCode> {
    let owner_id = user.sub;

    let result = Aliases::delete_many()
        .filter(aliases::Column::Id.eq(alias_id))
        .filter(aliases::Column::OwnerId.eq(owner_id))
        .exec(&*db)
        .await?;

    if result.rows_affected == 0 {
        return Err(AppError::NotFound(format!(
            "Alias {} not found or not owned by you",
            alias_id
        )));
    }

    Ok(StatusCode::NO_CONTENT)
}

// ── POST /api/entities/:scope/:id/confirm ────────────────────────────────────

pub async fn handle_confirm_entity(
    State(db): State<Arc<DatabaseConnection>>,
    ExtractUser(user): ExtractUser,
    Path((scope, entity_id)): Path<(String, Uuid)>,
) -> AppResult<Json<Value>> {
    let owner_id = user.sub;

    if scope_meta(&scope).is_none() {
        return Err(AppError::BadRequest(format!(
            "Unknown scope '{}'. Valid: category, merchant, product",
            scope
        )));
    }

    // actor has no review_state; the 3 fixed actors do not need review.
    if scope == "actor" {
        return Err(AppError::BadRequest(format!(
            "Scope '{}' does not support confirm (no review_state column)",
            scope
        )));
    }

    // 차감 protection (category scope only).
    if scope == "category" {
        check_chagang_protection(&*db, owner_id, entity_id, "entity").await?;
    }

    // Attempt to set review_state = 'confirmed'.
    let new_state = confirm_entity(&*db, owner_id, &scope, entity_id).await?;

    Ok(Json(json!({ "id": entity_id, "review_state": new_state })))
}

// ── Internal helpers ──────────────────────────────────────────────────────────

/// Verify that the given entity id exists in the correct scope table for this owner.
async fn verify_entity_exists(
    txn: &DatabaseTransaction,
    owner_id: Uuid,
    scope: &str,
    entity_id: Uuid,
) -> Result<(), AppError> {
    let exists = match scope {
        "category" => Categories::find_by_id(entity_id)
            .filter(categories::Column::OwnerId.eq(owner_id))
            .one(txn)
            .await?
            .is_some(),

        "merchant" => Merchants::find_by_id(entity_id)
            .filter(merchants::Column::OwnerId.eq(owner_id))
            .one(txn)
            .await?
            .is_some(),

        "payment_method" => PaymentMethods::find_by_id(entity_id)
            .filter(payment_methods::Column::OwnerId.eq(owner_id))
            .one(txn)
            .await?
            .is_some(),

        "actor" => LedgerActors::find_by_id(entity_id)
            .filter(ledger_actors::Column::OwnerId.eq(owner_id))
            .one(txn)
            .await?
            .is_some(),

        "product" => Products::find_by_id(entity_id)
            .filter(products::Column::OwnerId.eq(owner_id))
            .one(txn)
            .await?
            .is_some(),

        _ => return Err(AppError::BadRequest(format!("Unknown scope: {}", scope))),
    };

    if !exists {
        return Err(AppError::NotFound(format!(
            "Entity {} not found in scope '{}' for this owner",
            entity_id, scope
        )));
    }
    Ok(())
}

/// For category scope: reject if the entity is the protected "차감" category.
/// Works on both DatabaseTransaction and DatabaseConnection via ConnectionTrait.
async fn check_chagang_protection<C: ConnectionTrait>(
    conn: &C,
    owner_id: Uuid,
    entity_id: Uuid,
    role: &str,
) -> Result<(), AppError> {
    let row = Categories::find_by_id(entity_id)
        .filter(categories::Column::OwnerId.eq(owner_id))
        .one(conn)
        .await?;

    if let Some(cat) = row {
        if cat.name == "차감" {
            return Err(AppError::Conflict(serde_json::json!({
                "error": "deduction_protected",
                "message": format!(
                    "The {} entity is '차감' which cannot be modified or merged.",
                    role
                ),
            })));
        }
    }
    Ok(())
}

/// For payment_method scope: reject cross-actor merges when both sides have non-null actor_id
/// and they differ.
async fn check_payment_method_actor_compatibility(
    txn: &DatabaseTransaction,
    owner_id: Uuid,
    source_id: Uuid,
    target_id: Uuid,
) -> Result<(), AppError> {
    let source = PaymentMethods::find_by_id(source_id)
        .filter(payment_methods::Column::OwnerId.eq(owner_id))
        .one(txn)
        .await?;

    let target = PaymentMethods::find_by_id(target_id)
        .filter(payment_methods::Column::OwnerId.eq(owner_id))
        .one(txn)
        .await?;

    let src_actor = source.and_then(|r| r.actor_id);
    let tgt_actor = target.and_then(|r| r.actor_id);

    // Reject only when both are non-null AND different.
    if let (Some(sa), Some(ta)) = (src_actor, tgt_actor) {
        if sa != ta {
            // Fetch actor names for a helpful error message.
            let sa_name = LedgerActors::find_by_id(sa)
                .filter(ledger_actors::Column::OwnerId.eq(owner_id))
                .one(txn)
                .await?
                .map(|r| r.name)
                .unwrap_or_else(|| sa.to_string());

            let ta_name = LedgerActors::find_by_id(ta)
                .filter(ledger_actors::Column::OwnerId.eq(owner_id))
                .one(txn)
                .await?
                .map(|r| r.name)
                .unwrap_or_else(|| ta.to_string());

            return Err(AppError::Conflict(serde_json::json!({
                "error": "actor_mismatch",
                "message": format!(
                    "Cannot merge payment methods across actors: source belongs to '{}', target belongs to '{}'.",
                    sa_name, ta_name
                ),
                "source_actor": sa_name,
                "target_actor": ta_name,
            })));
        }
    }

    Ok(())
}

/// Acquire a row-level lock on the source entity row for the merge path.
/// This ensures concurrent merges of the same source entity serialize.
/// (Pattern E — lock_exclusive)
async fn lock_entity_row(
    txn: &DatabaseTransaction,
    owner_id: Uuid,
    scope: &str,
    entity_id: Uuid,
) -> Result<(), AppError> {
    match scope {
        "category" => {
            Categories::find_by_id(entity_id)
                .filter(categories::Column::OwnerId.eq(owner_id))
                .lock(LockType::Update)
                .one(txn)
                .await?;
        }
        "merchant" => {
            Merchants::find_by_id(entity_id)
                .filter(merchants::Column::OwnerId.eq(owner_id))
                .lock(LockType::Update)
                .one(txn)
                .await?;
        }
        "payment_method" => {
            PaymentMethods::find_by_id(entity_id)
                .filter(payment_methods::Column::OwnerId.eq(owner_id))
                .lock(LockType::Update)
                .one(txn)
                .await?;
        }
        "actor" => {
            LedgerActors::find_by_id(entity_id)
                .filter(ledger_actors::Column::OwnerId.eq(owner_id))
                .lock(LockType::Update)
                .one(txn)
                .await?;
        }
        "product" => {
            Products::find_by_id(entity_id)
                .filter(products::Column::OwnerId.eq(owner_id))
                .lock(LockType::Update)
                .one(txn)
                .await?;
        }
        _ => {}
    }
    Ok(())
}

/// UPDATE transactions to remap old_id → new_id for the given scope FK.
/// Returns number of rows updated.
/// Uses raw SQL with whitelisted column names (from scope_meta) for dynamic FK columns.
async fn remap_transactions(
    txn: &DatabaseTransaction,
    owner_id: Uuid,
    scope: &str,
    old_id: Uuid,
    new_id: Uuid,
) -> Result<i64, AppError> {
    let (_, tx_fk) = scope_meta(scope).unwrap();

    // Column name from scope_meta whitelist — not from user input. Safe.
    let sql = format!(
        "UPDATE transactions SET {} = $1 WHERE owner_id = $2 AND {} = $3",
        tx_fk, tx_fk
    );
    let result = txn
        .execute(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            &sql,
            [new_id.into(), owner_id.into(), old_id.into()],
        ))
        .await
        .map_err(AppError::Orm)?;

    Ok(result.rows_affected() as i64)
}

/// When merging merchants, products that hung off the source merchant(s) must follow
/// to the canonical merchant — otherwise `products_merchant_fk` blocks the orphan
/// delete. Two cases handled in order:
///   1. **Name collision** (a product with the same name already exists under the
///      canonical merchant): remap transactions from the source product to the
///      canonical product, delete the source product's aliases, then delete the
///      source product. This is effectively a product-merge induced by the
///      merchant-merge.
///   2. **No collision**: simply UPDATE `merchant_id` to the canonical id.
///
/// Caller must already hold the merge transaction; this helper does not commit.
/// Accepts a `Vec<Uuid>` of source merchant ids so the single-alias merge path
/// (one source) and the cluster bulk merge (N sources) can share one helper.
pub(crate) async fn cascade_merge_merchant_products(
    txn: &DatabaseTransaction,
    owner_id: Uuid,
    old_merchant_ids: Vec<Uuid>,
    new_merchant_id: Uuid,
) -> Result<(), AppError> {
    if old_merchant_ids.is_empty() {
        return Ok(());
    }

    // 1) Remap transactions for products that collide by name with a product on
    //    the canonical merchant. `t.product_id` becomes the canonical product's id.
    let remap_tx_sql = r#"
        UPDATE transactions t SET product_id = c.new_pid
        FROM (
            SELECT p_old.id AS old_pid, p_new.id AS new_pid
            FROM products p_old
            JOIN products p_new
              ON p_new.owner_id  = p_old.owner_id
             AND p_new.name      = p_old.name
             AND p_new.merchant_id = $1
            WHERE p_old.owner_id    = $2
              AND p_old.merchant_id = ANY($3)
        ) c
        WHERE t.owner_id = $2 AND t.product_id = c.old_pid
    "#;
    txn.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        remap_tx_sql,
        [
            new_merchant_id.into(),
            owner_id.into(),
            old_merchant_ids.clone().into(),
        ],
    ))
    .await
    .map_err(AppError::Orm)?;

    // 2) Delete aliases that pointed at the now-orphaned colliding products.
    let delete_aliases_sql = r#"
        DELETE FROM aliases
        WHERE owner_id = $1
          AND scope = 'product'
          AND target_id IN (
            SELECT p_old.id
            FROM products p_old
            JOIN products p_new
              ON p_new.owner_id  = p_old.owner_id
             AND p_new.name      = p_old.name
             AND p_new.merchant_id = $2
            WHERE p_old.owner_id    = $1
              AND p_old.merchant_id = ANY($3)
          )
    "#;
    txn.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        delete_aliases_sql,
        [
            owner_id.into(),
            new_merchant_id.into(),
            old_merchant_ids.clone().into(),
        ],
    ))
    .await
    .map_err(AppError::Orm)?;

    // 3) Delete the colliding source products themselves.
    let delete_products_sql = r#"
        DELETE FROM products p_old
        USING products p_new
        WHERE p_old.owner_id      = $1
          AND p_old.merchant_id   = ANY($2)
          AND p_new.owner_id      = p_old.owner_id
          AND p_new.merchant_id   = $3
          AND p_new.name          = p_old.name
    "#;
    txn.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        delete_products_sql,
        [
            owner_id.into(),
            old_merchant_ids.clone().into(),
            new_merchant_id.into(),
        ],
    ))
    .await
    .map_err(AppError::Orm)?;

    // 4) Re-parent the remaining (non-colliding) products to the canonical merchant.
    //    Safe under the partial unique index since collisions were already pruned.
    let remap_products_sql = r#"
        UPDATE products SET merchant_id = $1
        WHERE owner_id = $2 AND merchant_id = ANY($3)
    "#;
    txn.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        remap_products_sql,
        [
            new_merchant_id.into(),
            owner_id.into(),
            old_merchant_ids.into(),
        ],
    ))
    .await
    .map_err(AppError::Orm)?;

    Ok(())
}

/// Delete the source entity if no alias still points to it and no transaction references it.
/// Returns true if deleted.
async fn maybe_delete_orphan(
    txn: &DatabaseTransaction,
    owner_id: Uuid,
    scope: &str,
    entity_id: Uuid,
) -> Result<bool, AppError> {
    let (entity_table, tx_fk) = scope_meta(scope).unwrap();

    // Check alias references.
    #[derive(FromQueryResult)]
    struct CountRow { count: i64 }

    let alias_count = CountRow::find_by_statement(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT COUNT(*)::bigint AS count FROM aliases WHERE owner_id = $1 AND scope = $2 AND target_id = $3",
        [owner_id.into(), scope.into(), entity_id.into()],
    ))
    .one(txn)
    .await
    .map_err(AppError::Orm)?
    .map(|r| r.count)
    .unwrap_or(0);

    if alias_count > 0 {
        return Ok(false);
    }

    // Check transaction references using the scope FK column.
    // Column name comes from scope_meta() which is validated against a whitelist — safe.
    let tx_count_sql = format!(
        "SELECT COUNT(*)::bigint AS count FROM transactions WHERE owner_id = $1 AND {} = $2",
        tx_fk
    );
    let tx_count = CountRow::find_by_statement(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        &tx_count_sql,
        [owner_id.into(), entity_id.into()],
    ))
    .one(txn)
    .await
    .map_err(AppError::Orm)?
    .map(|r| r.count)
    .unwrap_or(0);

    if tx_count > 0 {
        return Ok(false);
    }

    // Safe to delete: no alias and no transaction references.
    // Table and column names are from the scope_meta whitelist — not from user input.
    let delete_sql = format!(
        "DELETE FROM {} WHERE id = $1 AND owner_id = $2",
        entity_table
    );
    txn.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        &delete_sql,
        [entity_id.into(), owner_id.into()],
    ))
    .await
    .map_err(AppError::Orm)?;

    Ok(true)
}

/// Set review_state = 'confirmed' for an entity, returning the resulting state.
/// Idempotent: re-confirming an already-confirmed entity returns 200 with 'confirmed'.
/// Returns Err(NotFound) if the entity does not exist for this owner.
/// Works on both DatabaseTransaction and DatabaseConnection via ConnectionTrait.
async fn confirm_entity<C: ConnectionTrait>(
    conn: &C,
    owner_id: Uuid,
    scope: &str,
    entity_id: Uuid,
) -> Result<String, AppError> {
    match scope {
        "category" => {
            let row = Categories::find_by_id(entity_id)
                .filter(categories::Column::OwnerId.eq(owner_id))
                .one(conn)
                .await?
                .ok_or_else(|| AppError::NotFound(format!("Category {} not found", entity_id)))?;

            let mut active: categories::ActiveModel = row.into();
            active.review_state = Set("confirmed".to_string());
            let updated = active.update(conn).await?;
            Ok(updated.review_state)
        }

        "merchant" => {
            let row = Merchants::find_by_id(entity_id)
                .filter(merchants::Column::OwnerId.eq(owner_id))
                .one(conn)
                .await?
                .ok_or_else(|| AppError::NotFound(format!("Merchant {} not found", entity_id)))?;

            let mut active: merchants::ActiveModel = row.into();
            active.review_state = Set("confirmed".to_string());
            let updated = active.update(conn).await?;
            Ok(updated.review_state)
        }

        "product" => {
            let row = Products::find_by_id(entity_id)
                .filter(products::Column::OwnerId.eq(owner_id))
                .one(conn)
                .await?
                .ok_or_else(|| AppError::NotFound(format!("Product {} not found", entity_id)))?;

            let mut active: products::ActiveModel = row.into();
            active.review_state = Set("confirmed".to_string());
            let updated = active.update(conn).await?;
            Ok(updated.review_state)
        }

        "payment_method" => {
            let row = PaymentMethods::find_by_id(entity_id)
                .filter(payment_methods::Column::OwnerId.eq(owner_id))
                .one(conn)
                .await?
                .ok_or_else(|| AppError::NotFound(format!("Payment method {} not found", entity_id)))?;

            let mut active: payment_methods::ActiveModel = row.into();
            active.review_state = Set("confirmed".to_string());
            let updated = active.update(conn).await?;
            Ok(updated.review_state)
        }

        _ => Err(AppError::BadRequest(format!(
            "Scope '{}' does not support confirm",
            scope
        ))),
    }
}

// ── Unit tests for levenshtein ────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::levenshtein;

    #[test]
    fn levenshtein_basic() {
        assert_eq!(levenshtein("", ""), 0);
        assert_eq!(levenshtein("abc", "abc"), 0);
        assert_eq!(levenshtein("이 마트", "이마트"), 1); // 공백 삭제 1회
        assert_eq!(levenshtein("abc", ""), 3);
        assert_eq!(levenshtein("", "abc"), 3);
        assert_eq!(levenshtein("kitten", "sitting"), 3);
    }
}
