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
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{PgConnection, PgPool};
use std::sync::Arc;
use uuid::Uuid;

use crate::auth::ExtractUser;
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
    pub raw_texts: Vec<AliasInfo>,
    pub merge_candidates: Vec<MergeCandidate>,
}

#[derive(Debug, Serialize)]
pub struct MergeCandidate {
    pub id: Uuid,
    pub name: String,
}

pub async fn handle_get_review_queue(
    State(pool): State<Arc<PgPool>>,
    ExtractUser(user): ExtractUser,
    Query(params): Query<ReviewQueueQuery>,
) -> AppResult<Json<Vec<ReviewQueueItem>>> {
    let owner_id = user.sub;
    let mut conn = pool.acquire().await?;

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
        let items = review_queue_for_scope(&mut conn, owner_id, scope).await?;
        result.extend(items);
    }

    Ok(Json(result))
}

/// Fetch pending entities for a given scope and build review-queue items with
/// their aliases and merge candidates.
async fn review_queue_for_scope(
    conn: &mut PgConnection,
    owner_id: Uuid,
    scope: &str,
) -> Result<Vec<ReviewQueueItem>, AppError> {
    // Fetch all entities of this scope for this owner (needed for merge candidates too).
    // We always fetch all, because merge_candidates requires the full list.
    let all_entities = fetch_all_entities(conn, owner_id, scope).await?;

    // Fetch all aliases for this scope+owner (to join raw_texts).
    let all_aliases: Vec<(Uuid, String, String, Uuid)> = sqlx::query!(
        r#"
        SELECT id AS "alias_id!: Uuid",
               raw_text AS "raw_text!",
               norm_key AS "norm_key!",
               target_id AS "target_id!: Uuid"
        FROM aliases
        WHERE owner_id = $1 AND scope = $2
        "#,
        owner_id,
        scope,
    )
    .fetch_all(&mut *conn)
    .await?
    .into_iter()
    .map(|r| (r.alias_id, r.raw_text, r.norm_key, r.target_id))
    .collect();

    // Only work on pending entities.
    let pending: Vec<(Uuid, String)> = all_entities
        .iter()
        .filter(|(_, _, rs)| rs == "pending")
        .map(|(id, name, _)| (*id, name.clone()))
        .collect();

    let mut items = Vec::with_capacity(pending.len());

    for (entity_id, entity_name) in &pending {
        // Collect aliases that point to this entity.
        let raw_texts: Vec<AliasInfo> = all_aliases
            .iter()
            .filter(|(_, _, _, tid)| tid == entity_id)
            .map(|(aid, rt, nk, _)| AliasInfo {
                alias_id: *aid,
                raw_text: rt.clone(),
                norm_key: nk.clone(),
            })
            .collect();

        // norm_keys for this entity (from aliases).
        let my_norm_keys: Vec<String> = raw_texts.iter().map(|a| a.norm_key.clone()).collect();

        // Merge candidates: other entities (same owner, same scope, distinct id) that share
        // any norm_key OR are within Levenshtein ≤ 1 on the canonical name.
        let entity_name_lower = entity_name.to_lowercase();
        let merge_candidates: Vec<MergeCandidate> = all_entities
            .iter()
            .filter(|(other_id, other_name, _)| {
                if other_id == entity_id {
                    return false;
                }
                // Shared norm_key: any of the target entity's aliases has the same norm_key.
                let other_norms: Vec<&str> = all_aliases
                    .iter()
                    .filter(|(_, _, _, tid)| tid == other_id)
                    .map(|(_, _, nk, _)| nk.as_str())
                    .collect();

                let shares_norm_key = my_norm_keys
                    .iter()
                    .any(|nk| other_norms.contains(&nk.as_str()));

                if shares_norm_key {
                    return true;
                }

                // Levenshtein ≤ 1 on canonical name (case-insensitive).
                let dist = levenshtein(&entity_name_lower, &other_name.to_lowercase());
                dist <= 1
            })
            .map(|(id, name, _)| MergeCandidate {
                id: *id,
                name: name.clone(),
            })
            .collect();

        items.push(ReviewQueueItem {
            scope: scope.to_string(),
            id: *entity_id,
            name: entity_name.clone(),
            review_state: "pending".to_string(),
            raw_texts,
            merge_candidates,
        });
    }

    Ok(items)
}

/// Returns (id, name, review_state) for all entities of the given scope.
async fn fetch_all_entities(
    conn: &mut PgConnection,
    owner_id: Uuid,
    scope: &str,
) -> Result<Vec<(Uuid, String, String)>, AppError> {
    let rows = match scope {
        "category" => sqlx::query!(
            r#"SELECT id AS "id!: Uuid", name AS "name!", review_state AS "review_state!"
               FROM categories WHERE owner_id = $1 ORDER BY name"#,
            owner_id
        )
        .fetch_all(&mut *conn)
        .await?
        .into_iter()
        .map(|r| (r.id, r.name, r.review_state))
        .collect(),

        "merchant" => sqlx::query!(
            r#"SELECT id AS "id!: Uuid", name AS "name!", review_state AS "review_state!"
               FROM merchants WHERE owner_id = $1 ORDER BY name"#,
            owner_id
        )
        .fetch_all(&mut *conn)
        .await?
        .into_iter()
        .map(|r| (r.id, r.name, r.review_state))
        .collect(),

        "payment_method" => {
            // payment_methods has no review_state column — treat all as 'pending' for query
            // symmetry; the confirm endpoint handles this scope by setting a synthetic state.
            // Actually: schema does NOT have review_state on payment_methods; the review queue
            // only makes sense for scopes that have that column.
            // Return empty — payment_method does not participate in the review queue via
            // this mechanism (they are mapped by actor assignment, Step C).
            // Per spec: actor scope has only 3 fixed values → usually empty. Same logic.
            vec![]
        }

        "actor" => {
            // ledger_actors has no review_state either. Return empty for symmetry.
            vec![]
        }

        "product" => sqlx::query!(
            r#"SELECT id AS "id!: Uuid", name AS "name!", review_state AS "review_state!"
               FROM products WHERE owner_id = $1 ORDER BY name"#,
            owner_id
        )
        .fetch_all(&mut *conn)
        .await?
        .into_iter()
        .map(|r| (r.id, r.name, r.review_state))
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
}

#[derive(Serialize)]
pub struct PostAliasResponse {
    pub created: bool,
    pub remapped_transaction_count: i64,
    pub orphan_deleted: bool,
}

pub async fn handle_post_alias(
    State(pool): State<Arc<PgPool>>,
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

    let mut tx = pool.begin().await?;

    // Verify target entity exists for this owner+scope.
    verify_entity_exists(&mut tx, owner_id, scope, body.target_id).await?;

    // Check for 차감 protection (category scope).
    if scope == "category" {
        check_chagang_protection(&mut tx, owner_id, body.target_id, "target").await?;
    }

    // Look up existing alias for (owner_id, scope, norm_key).
    let existing = sqlx::query!(
        r#"
        SELECT id AS "alias_id!: Uuid", target_id AS "target_id!: Uuid"
        FROM aliases
        WHERE owner_id = $1 AND scope = $2 AND norm_key = $3
        "#,
        owner_id,
        scope.as_str(),
        norm,
    )
    .fetch_optional(&mut *tx)
    .await?;

    let resp = match existing {
        None => {
            // Create path: no existing alias.
            sqlx::query!(
                r#"INSERT INTO aliases (owner_id, scope, raw_text, norm_key, target_id)
                   VALUES ($1, $2, $3, $4, $5)"#,
                owner_id,
                scope.as_str(),
                body.raw_text,
                norm,
                body.target_id,
            )
            .execute(&mut *tx)
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
            let alias_id = existing_row.alias_id;

            // 차감 protection for source (category scope).
            if scope == "category" {
                check_chagang_protection(&mut tx, owner_id, old_target_id, "source").await?;
            }

            // payment_method cross-actor check.
            if scope == "payment_method" {
                check_payment_method_actor_compatibility(
                    &mut tx,
                    owner_id,
                    old_target_id,
                    body.target_id,
                )
                .await?;
            }

            // SELECT ... FOR UPDATE on the source entity row to prevent concurrent merges.
            // If another transaction already merged old_target_id into a different target,
            // we detect it by re-reading the alias after the lock.
            lock_entity_row(&mut tx, owner_id, scope, old_target_id).await?;

            // Re-read the alias under the lock to detect concurrent change.
            let alias_now = sqlx::query!(
                r#"
                SELECT target_id AS "target_id!: Uuid"
                FROM aliases
                WHERE id = $1 AND owner_id = $2
                "#,
                alias_id,
                owner_id,
            )
            .fetch_optional(&mut *tx)
            .await?;

            match alias_now {
                None => {
                    // Alias was deleted by concurrent operation.
                    return Err(AppError::Conflict(
                        "alias_changed: alias was removed by a concurrent operation".to_string(),
                    ));
                }
                Some(ref now) if now.target_id != old_target_id => {
                    // Another merge already remapped this alias.
                    return Err(AppError::Conflict(
                        "alias_changed: a concurrent merge already remapped this alias".to_string(),
                    ));
                }
                Some(_) => {} // Still points to old_target_id — we're the winner, proceed.
            }

            // Update alias to point to new target.
            sqlx::query!(
                r#"UPDATE aliases SET target_id = $1, raw_text = $2 WHERE id = $3"#,
                body.target_id,
                body.raw_text,
                alias_id,
            )
            .execute(&mut *tx)
            .await?;

            // Remap transactions.
            let remapped = remap_transactions(
                &mut tx,
                owner_id,
                scope,
                old_target_id,
                body.target_id,
            )
            .await?;

            // Optionally delete the orphaned source entity.
            let orphan_deleted =
                maybe_delete_orphan(&mut tx, owner_id, scope, old_target_id).await?;

            PostAliasResponse {
                created: false,
                remapped_transaction_count: remapped,
                orphan_deleted,
            }
        }
    };

    tx.commit().await?;
    Ok(Json(resp))
}

// ── DELETE /api/aliases/:id ───────────────────────────────────────────────────

pub async fn handle_delete_alias(
    State(pool): State<Arc<PgPool>>,
    ExtractUser(user): ExtractUser,
    Path(alias_id): Path<Uuid>,
) -> AppResult<StatusCode> {
    let owner_id = user.sub;

    let result = sqlx::query!(
        r#"DELETE FROM aliases WHERE id = $1 AND owner_id = $2"#,
        alias_id,
        owner_id,
    )
    .execute(&*pool)
    .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound(format!(
            "Alias {} not found or not owned by you",
            alias_id
        )));
    }

    Ok(StatusCode::NO_CONTENT)
}

// ── POST /api/entities/:scope/:id/confirm ────────────────────────────────────

pub async fn handle_confirm_entity(
    State(pool): State<Arc<PgPool>>,
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

    // Only category, merchant, product have review_state.
    if scope == "payment_method" || scope == "actor" {
        return Err(AppError::BadRequest(format!(
            "Scope '{}' does not support confirm (no review_state column)",
            scope
        )));
    }

    let mut conn = pool.acquire().await?;

    // 차감 protection (category scope only).
    if scope == "category" {
        check_chagang_protection(&mut conn, owner_id, entity_id, "entity").await?;
    }

    // Attempt to set review_state = 'confirmed'.
    let new_state = confirm_entity(&mut conn, owner_id, &scope, entity_id).await?;

    Ok(Json(json!({ "id": entity_id, "review_state": new_state })))
}

// ── Internal helpers ──────────────────────────────────────────────────────────

/// Verify that the given entity id exists in the correct scope table for this owner.
async fn verify_entity_exists(
    conn: &mut PgConnection,
    owner_id: Uuid,
    scope: &str,
    entity_id: Uuid,
) -> Result<(), AppError> {
    let exists = match scope {
        "category" => sqlx::query_scalar!(
            r#"SELECT EXISTS(SELECT 1 FROM categories WHERE id = $1 AND owner_id = $2) AS "e!: bool""#,
            entity_id, owner_id
        )
        .fetch_one(&mut *conn)
        .await?,

        "merchant" => sqlx::query_scalar!(
            r#"SELECT EXISTS(SELECT 1 FROM merchants WHERE id = $1 AND owner_id = $2) AS "e!: bool""#,
            entity_id, owner_id
        )
        .fetch_one(&mut *conn)
        .await?,

        "payment_method" => sqlx::query_scalar!(
            r#"SELECT EXISTS(SELECT 1 FROM payment_methods WHERE id = $1 AND owner_id = $2) AS "e!: bool""#,
            entity_id, owner_id
        )
        .fetch_one(&mut *conn)
        .await?,

        "actor" => sqlx::query_scalar!(
            r#"SELECT EXISTS(SELECT 1 FROM ledger_actors WHERE id = $1 AND owner_id = $2) AS "e!: bool""#,
            entity_id, owner_id
        )
        .fetch_one(&mut *conn)
        .await?,

        "product" => sqlx::query_scalar!(
            r#"SELECT EXISTS(SELECT 1 FROM products WHERE id = $1 AND owner_id = $2) AS "e!: bool""#,
            entity_id, owner_id
        )
        .fetch_one(&mut *conn)
        .await?,

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
async fn check_chagang_protection(
    conn: &mut PgConnection,
    owner_id: Uuid,
    entity_id: Uuid,
    role: &str,
) -> Result<(), AppError> {
    let name: Option<String> = sqlx::query_scalar!(
        r#"SELECT name FROM categories WHERE id = $1 AND owner_id = $2"#,
        entity_id,
        owner_id
    )
    .fetch_optional(&mut *conn)
    .await?;

    if let Some(n) = name {
        if n == "차감" {
            return Err(AppError::Conflict(format!(
                "Protected entity: the {} entity is '차감' which cannot be modified or merged",
                role
            )));
        }
    }
    Ok(())
}

/// For payment_method scope: reject cross-actor merges when both sides have non-null actor_id
/// and they differ.
async fn check_payment_method_actor_compatibility(
    conn: &mut PgConnection,
    owner_id: Uuid,
    source_id: Uuid,
    target_id: Uuid,
) -> Result<(), AppError> {
    let source_actor: Option<Option<Uuid>> = sqlx::query_scalar!(
        r#"SELECT actor_id FROM payment_methods WHERE id = $1 AND owner_id = $2"#,
        source_id,
        owner_id
    )
    .fetch_optional(&mut *conn)
    .await?;

    let target_actor: Option<Option<Uuid>> = sqlx::query_scalar!(
        r#"SELECT actor_id FROM payment_methods WHERE id = $1 AND owner_id = $2"#,
        target_id,
        owner_id
    )
    .fetch_optional(&mut *conn)
    .await?;

    // Flatten: None row → None actor_id; Some(row) → actor_id from row.
    let src_actor = source_actor.flatten();
    let tgt_actor = target_actor.flatten();

    // Reject only when both are non-null AND different.
    if let (Some(sa), Some(ta)) = (src_actor, tgt_actor) {
        if sa != ta {
            // Fetch actor names for a helpful error message.
            let sa_name: String = sqlx::query_scalar!(
                r#"SELECT name FROM ledger_actors WHERE id = $1 AND owner_id = $2"#,
                sa, owner_id
            )
            .fetch_optional(&mut *conn)
            .await?
            .unwrap_or_else(|| sa.to_string());

            let ta_name: String = sqlx::query_scalar!(
                r#"SELECT name FROM ledger_actors WHERE id = $1 AND owner_id = $2"#,
                ta, owner_id
            )
            .fetch_optional(&mut *conn)
            .await?
            .unwrap_or_else(|| ta.to_string());

            return Err(AppError::Conflict(format!(
                "Cannot merge payment methods across actors: source belongs to '{}', target belongs to '{}'",
                sa_name, ta_name
            )));
        }
    }

    Ok(())
}

/// Acquire a row-level lock on the source entity row for the merge path.
/// This ensures concurrent merges of the same source entity serialize.
async fn lock_entity_row(
    conn: &mut PgConnection,
    owner_id: Uuid,
    scope: &str,
    entity_id: Uuid,
) -> Result<(), AppError> {
    match scope {
        "category" => {
            sqlx::query!(
                r#"SELECT id FROM categories WHERE id = $1 AND owner_id = $2 FOR UPDATE"#,
                entity_id, owner_id
            )
            .fetch_optional(&mut *conn)
            .await?;
        }
        "merchant" => {
            sqlx::query!(
                r#"SELECT id FROM merchants WHERE id = $1 AND owner_id = $2 FOR UPDATE"#,
                entity_id, owner_id
            )
            .fetch_optional(&mut *conn)
            .await?;
        }
        "payment_method" => {
            sqlx::query!(
                r#"SELECT id FROM payment_methods WHERE id = $1 AND owner_id = $2 FOR UPDATE"#,
                entity_id, owner_id
            )
            .fetch_optional(&mut *conn)
            .await?;
        }
        "actor" => {
            sqlx::query!(
                r#"SELECT id FROM ledger_actors WHERE id = $1 AND owner_id = $2 FOR UPDATE"#,
                entity_id, owner_id
            )
            .fetch_optional(&mut *conn)
            .await?;
        }
        "product" => {
            sqlx::query!(
                r#"SELECT id FROM products WHERE id = $1 AND owner_id = $2 FOR UPDATE"#,
                entity_id, owner_id
            )
            .fetch_optional(&mut *conn)
            .await?;
        }
        _ => {}
    }
    Ok(())
}

/// UPDATE transactions to remap old_id → new_id for the given scope FK.
/// Returns number of rows updated.
async fn remap_transactions(
    conn: &mut PgConnection,
    owner_id: Uuid,
    scope: &str,
    old_id: Uuid,
    new_id: Uuid,
) -> Result<i64, AppError> {
    let rows_affected = match scope {
        "category" => sqlx::query!(
            r#"UPDATE transactions SET category_id = $1
               WHERE owner_id = $2 AND category_id = $3"#,
            new_id, owner_id, old_id
        )
        .execute(&mut *conn)
        .await?
        .rows_affected(),

        "merchant" => sqlx::query!(
            r#"UPDATE transactions SET merchant_id = $1
               WHERE owner_id = $2 AND merchant_id = $3"#,
            new_id, owner_id, old_id
        )
        .execute(&mut *conn)
        .await?
        .rows_affected(),

        "payment_method" => sqlx::query!(
            r#"UPDATE transactions SET payment_method_id = $1
               WHERE owner_id = $2 AND payment_method_id = $3"#,
            new_id, owner_id, old_id
        )
        .execute(&mut *conn)
        .await?
        .rows_affected(),

        "actor" => sqlx::query!(
            r#"UPDATE transactions SET actor_id = $1
               WHERE owner_id = $2 AND actor_id = $3"#,
            new_id, owner_id, old_id
        )
        .execute(&mut *conn)
        .await?
        .rows_affected(),

        "product" => sqlx::query!(
            r#"UPDATE transactions SET product_id = $1
               WHERE owner_id = $2 AND product_id = $3"#,
            new_id, owner_id, old_id
        )
        .execute(&mut *conn)
        .await?
        .rows_affected(),

        _ => 0,
    };

    Ok(rows_affected as i64)
}

/// Delete the source entity if no alias still points to it and no transaction references it.
/// Returns true if deleted.
async fn maybe_delete_orphan(
    conn: &mut PgConnection,
    owner_id: Uuid,
    scope: &str,
    entity_id: Uuid,
) -> Result<bool, AppError> {
    let (entity_table, tx_fk) = scope_meta(scope).unwrap();

    // Check alias references.
    let alias_count: i64 = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM aliases WHERE owner_id = $1 AND scope = $2 AND target_id = $3",
    )
    .bind(owner_id)
    .bind(scope)
    .bind(entity_id)
    .fetch_one(&mut *conn)
    .await
    .map_err(AppError::Database)?;

    if alias_count > 0 {
        return Ok(false);
    }

    // Check transaction references using the scope FK column.
    // We use a dynamic query here because the FK column name varies by scope.
    // The column name comes from scope_meta() which is validated against a whitelist — safe.
    let tx_count_sql = format!(
        "SELECT COUNT(*) FROM transactions WHERE owner_id = $1 AND {} = $2",
        tx_fk
    );
    let tx_count: i64 = sqlx::query_scalar::<_, i64>(&tx_count_sql)
        .bind(owner_id)
        .bind(entity_id)
        .fetch_one(&mut *conn)
        .await
        .map_err(AppError::Database)?;

    if tx_count > 0 {
        return Ok(false);
    }

    // Safe to delete: no alias and no transaction references.
    // Column names are from the scope_meta whitelist — not from user input.
    let delete_sql = format!(
        "DELETE FROM {} WHERE id = $1 AND owner_id = $2",
        entity_table
    );
    sqlx::query(&delete_sql)
        .bind(entity_id)
        .bind(owner_id)
        .execute(&mut *conn)
        .await
        .map_err(AppError::Database)?;

    Ok(true)
}

/// Set review_state = 'confirmed' for an entity, returning the resulting state.
/// Idempotent: re-confirming an already-confirmed entity returns 200 with 'confirmed'.
/// Returns Err(NotFound) if the entity does not exist for this owner.
async fn confirm_entity(
    conn: &mut PgConnection,
    owner_id: Uuid,
    scope: &str,
    entity_id: Uuid,
) -> Result<String, AppError> {
    match scope {
        "category" => {
            let row = sqlx::query!(
                r#"UPDATE categories SET review_state = 'confirmed'
                   WHERE id = $1 AND owner_id = $2
                   RETURNING review_state AS "rs!""#,
                entity_id,
                owner_id,
            )
            .fetch_optional(&mut *conn)
            .await?;
            row.map(|r| r.rs)
                .ok_or_else(|| AppError::NotFound(format!("Category {} not found", entity_id)))
        }

        "merchant" => {
            let row = sqlx::query!(
                r#"UPDATE merchants SET review_state = 'confirmed'
                   WHERE id = $1 AND owner_id = $2
                   RETURNING review_state AS "rs!""#,
                entity_id,
                owner_id,
            )
            .fetch_optional(&mut *conn)
            .await?;
            row.map(|r| r.rs)
                .ok_or_else(|| AppError::NotFound(format!("Merchant {} not found", entity_id)))
        }

        "product" => {
            let row = sqlx::query!(
                r#"UPDATE products SET review_state = 'confirmed'
                   WHERE id = $1 AND owner_id = $2
                   RETURNING review_state AS "rs!""#,
                entity_id,
                owner_id,
            )
            .fetch_optional(&mut *conn)
            .await?;
            row.map(|r| r.rs)
                .ok_or_else(|| AppError::NotFound(format!("Product {} not found", entity_id)))
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
