use anyhow::{Context, Result};
use rust_decimal::Decimal;
use sqlx::PgConnection;
use uuid::Uuid;

use crate::domain::{IntegrityWarning, RawRow, TransactionRow, UnresolvedAlias};
use crate::import::normalize::to_norm_key;

// ── Atomic upsert helpers ────────────────────────────────────────────────────
//
// Pattern: INSERT ... ON CONFLICT (<partial-index cols>) WHERE <predicate> DO NOTHING RETURNING id
//          → if None (conflict), SELECT id WHERE <same predicate>
//
// Partial unique indexes (see 001_init.sql) handle NULL-in-unique-constraint gaps for
// categories (parent_id IS NULL) and products (merchant_id IS NOT NULL / IS NULL).

/// Atomic upsert for categories (root-level only; all pipeline categories have parent_id IS NULL).
///
/// The partial index `categories_owner_name_root_uniq` on (owner_id, name) WHERE parent_id IS NULL
/// makes ON CONFLICT safe even when parent_id is NULL — no advisory lock needed.
///
/// "차감" rule: norm == "차감" gets review_state='confirmed' on first creation.
/// DO NOTHING ensures an existing 'confirmed' row is never downgraded.
async fn upsert_category(
    conn: &mut PgConnection,
    owner_id: Uuid,
    raw_text: &str,
) -> Result<(Uuid, bool)> {
    let norm = to_norm_key(raw_text);

    // 1. Alias lookup (cheapest path).
    let existing = sqlx::query!(
        r#"SELECT target_id FROM aliases WHERE owner_id = $1 AND scope = 'category' AND norm_key = $2"#,
        owner_id,
        norm
    )
    .fetch_optional(&mut *conn)
    .await?;

    if let Some(row) = existing {
        return Ok((row.target_id, false));
    }

    // "차감" (normalized) always gets review_state='confirmed' on first creation.
    let is_deduction = norm == "차감";
    let review_state = if is_deduction { "confirmed" } else { "pending" };

    // 2. INSERT targeting the partial index; fallback SELECT on conflict.
    let cat_id_opt: Option<Uuid> = sqlx::query_scalar!(
        r#"INSERT INTO categories (owner_id, name, kind, review_state)
           VALUES ($1, $2, 'expense', $3)
           ON CONFLICT (owner_id, name) WHERE parent_id IS NULL DO NOTHING
           RETURNING id"#,
        owner_id,
        norm,
        review_state,
    )
    .fetch_optional(&mut *conn)
    .await
    .context("category INSERT failed")?;

    let (cat_id, is_new) = match cat_id_opt {
        Some(id) => (id, true),
        None => {
            let id = sqlx::query_scalar!(
                r#"SELECT id FROM categories WHERE owner_id = $1 AND name = $2 AND parent_id IS NULL"#,
                owner_id,
                norm
            )
            .fetch_one(&mut *conn)
            .await
            .context("category conflict-fallback SELECT failed")?;
            (id, false)
        }
    };

    // 3. Register alias.
    sqlx::query!(
        r#"INSERT INTO aliases (owner_id, scope, raw_text, norm_key, target_id)
           VALUES ($1, 'category', $2, $3, $4)
           ON CONFLICT (owner_id, scope, norm_key) DO NOTHING"#,
        owner_id,
        raw_text,
        norm,
        cat_id,
    )
    .execute(&mut *conn)
    .await?;

    Ok((cat_id, is_new))
}

async fn upsert_merchant(
    conn: &mut PgConnection,
    owner_id: Uuid,
    raw_text: &str,
) -> Result<(Uuid, bool)> {
    let norm = to_norm_key(raw_text);

    // 1. Alias lookup.
    let existing = sqlx::query!(
        r#"SELECT target_id FROM aliases WHERE owner_id = $1 AND scope = 'merchant' AND norm_key = $2"#,
        owner_id,
        norm
    )
    .fetch_optional(&mut *conn)
    .await?;

    if let Some(row) = existing {
        return Ok((row.target_id, false));
    }

    // 2. Atomic INSERT with fallback SELECT.
    //    merchants.UNIQUE (owner_id, name) has no NULLs — ON CONFLICT works directly.
    let merch_id: Option<Uuid> = sqlx::query_scalar!(
        r#"INSERT INTO merchants (owner_id, name, review_state)
           VALUES ($1, $2, 'pending')
           ON CONFLICT (owner_id, name) DO NOTHING
           RETURNING id"#,
        owner_id,
        norm,
    )
    .fetch_optional(&mut *conn)
    .await?;

    let (merch_id, is_new) = match merch_id {
        Some(id) => (id, true),
        None => {
            let id = sqlx::query_scalar!(
                r#"SELECT id FROM merchants WHERE owner_id = $1 AND name = $2"#,
                owner_id,
                norm
            )
            .fetch_one(&mut *conn)
            .await
            .context("merchant conflict-fallback SELECT failed")?;
            (id, false)
        }
    };

    sqlx::query!(
        r#"INSERT INTO aliases (owner_id, scope, raw_text, norm_key, target_id)
           VALUES ($1, 'merchant', $2, $3, $4)
           ON CONFLICT (owner_id, scope, norm_key) DO NOTHING"#,
        owner_id,
        raw_text,
        norm,
        merch_id,
    )
    .execute(&mut *conn)
    .await?;

    Ok((merch_id, is_new))
}

async fn upsert_actor(
    conn: &mut PgConnection,
    owner_id: Uuid,
    raw_text: &str,
) -> Result<(Uuid, bool)> {
    let norm = to_norm_key(raw_text);

    let existing = sqlx::query!(
        r#"SELECT target_id FROM aliases WHERE owner_id = $1 AND scope = 'actor' AND norm_key = $2"#,
        owner_id,
        norm
    )
    .fetch_optional(&mut *conn)
    .await?;

    if let Some(row) = existing {
        return Ok((row.target_id, false));
    }

    // ledger_actors.UNIQUE (owner_id, name) has no NULLs — ON CONFLICT works directly.
    let actor_id: Option<Uuid> = sqlx::query_scalar!(
        r#"INSERT INTO ledger_actors (owner_id, name) VALUES ($1, $2)
           ON CONFLICT (owner_id, name) DO NOTHING
           RETURNING id"#,
        owner_id,
        norm,
    )
    .fetch_optional(&mut *conn)
    .await?;

    let (actor_id, is_new) = match actor_id {
        Some(id) => (id, true),
        None => {
            let id = sqlx::query_scalar!(
                r#"SELECT id FROM ledger_actors WHERE owner_id = $1 AND name = $2"#,
                owner_id,
                norm
            )
            .fetch_one(&mut *conn)
            .await
            .context("actor conflict-fallback SELECT failed")?;
            (id, false)
        }
    };

    sqlx::query!(
        r#"INSERT INTO aliases (owner_id, scope, raw_text, norm_key, target_id)
           VALUES ($1, 'actor', $2, $3, $4)
           ON CONFLICT (owner_id, scope, norm_key) DO NOTHING"#,
        owner_id,
        raw_text,
        norm,
        actor_id,
    )
    .execute(&mut *conn)
    .await?;

    Ok((actor_id, is_new))
}

async fn upsert_payment_method(
    conn: &mut PgConnection,
    owner_id: Uuid,
    raw_text: &str,
) -> Result<(Uuid, bool)> {
    let norm = to_norm_key(raw_text);

    let existing = sqlx::query!(
        r#"SELECT target_id FROM aliases WHERE owner_id = $1 AND scope = 'payment_method' AND norm_key = $2"#,
        owner_id,
        norm
    )
    .fetch_optional(&mut *conn)
    .await?;

    if let Some(row) = existing {
        return Ok((row.target_id, false));
    }

    // payment_methods.UNIQUE (owner_id, name) has no NULLs — ON CONFLICT works directly.
    let pm_id: Option<Uuid> = sqlx::query_scalar!(
        r#"INSERT INTO payment_methods (owner_id, name) VALUES ($1, $2)
           ON CONFLICT (owner_id, name) DO NOTHING
           RETURNING id"#,
        owner_id,
        norm,
    )
    .fetch_optional(&mut *conn)
    .await?;

    let (pm_id, is_new) = match pm_id {
        Some(id) => (id, true),
        None => {
            let id = sqlx::query_scalar!(
                r#"SELECT id FROM payment_methods WHERE owner_id = $1 AND name = $2"#,
                owner_id,
                norm
            )
            .fetch_one(&mut *conn)
            .await
            .context("payment_method conflict-fallback SELECT failed")?;
            (id, false)
        }
    };

    sqlx::query!(
        r#"INSERT INTO aliases (owner_id, scope, raw_text, norm_key, target_id)
           VALUES ($1, 'payment_method', $2, $3, $4)
           ON CONFLICT (owner_id, scope, norm_key) DO NOTHING"#,
        owner_id,
        raw_text,
        norm,
        pm_id,
    )
    .execute(&mut *conn)
    .await?;

    Ok((pm_id, is_new))
}

/// Product upsert (non-NULL merchant_id path only — callers always pass Some(merch_id)).
///
/// The partial index `products_owner_merchant_name_uniq` on (owner_id, merchant_id, name)
/// WHERE merchant_id IS NOT NULL enables ON CONFLICT without an advisory lock.
async fn upsert_product(
    conn: &mut PgConnection,
    owner_id: Uuid,
    merchant_id: Uuid,
    memo: &str,
) -> Result<(Uuid, bool)> {
    let norm = to_norm_key(memo);

    // Product aliases are keyed only on norm_key (not merchant), matching M1 behavior.
    let existing = sqlx::query!(
        r#"SELECT target_id FROM aliases WHERE owner_id = $1 AND scope = 'product' AND norm_key = $2"#,
        owner_id,
        norm
    )
    .fetch_optional(&mut *conn)
    .await?;

    if let Some(row) = existing {
        return Ok((row.target_id, false));
    }

    // INSERT targeting the partial index; fallback SELECT on conflict.
    let prod_id: Option<Uuid> = sqlx::query_scalar!(
        r#"INSERT INTO products (owner_id, merchant_id, name, review_state)
           VALUES ($1, $2, $3, 'pending')
           ON CONFLICT (owner_id, merchant_id, name) WHERE merchant_id IS NOT NULL DO NOTHING
           RETURNING id"#,
        owner_id,
        merchant_id,
        norm,
    )
    .fetch_optional(&mut *conn)
    .await?;

    let (prod_id, is_new) = match prod_id {
        Some(id) => (id, true),
        None => {
            let id = sqlx::query_scalar!(
                r#"SELECT id FROM products WHERE owner_id = $1 AND merchant_id = $2 AND name = $3"#,
                owner_id,
                merchant_id,
                norm
            )
            .fetch_one(&mut *conn)
            .await
            .context("product conflict-fallback SELECT failed")?;
            (id, false)
        }
    };

    sqlx::query!(
        r#"INSERT INTO aliases (owner_id, scope, raw_text, norm_key, target_id)
           VALUES ($1, 'product', $2, $3, $4)
           ON CONFLICT (owner_id, scope, norm_key) DO NOTHING"#,
        owner_id,
        memo,
        norm,
        prod_id,
    )
    .execute(&mut *conn)
    .await?;

    Ok((prod_id, is_new))
}

/// Insert one row into transactions_raw.
async fn insert_raw(
    conn: &mut PgConnection,
    owner_id: Uuid,
    batch_id: Uuid,
    row: &RawRow,
) -> Result<Uuid> {
    let raw_id: Uuid = sqlx::query_scalar!(
        r#"INSERT INTO transactions_raw (
            owner_id, import_batch_id, row_index, group_id, is_group_header,
            occurred_on, raw_date_serial, merchant_text, actor_text, category_text,
            total_amount, memo, unit_price, quantity, line_amount,
            payment_text, evidence_text, extras
        ) VALUES (
            $1, $2, $3, $4, $5,
            $6, $7, $8, $9, $10,
            $11, $12, $13, $14, $15,
            $16, $17, $18
        ) RETURNING id"#,
        owner_id,
        batch_id,
        row.row_index,
        row.group_id,
        row.is_group_header,
        row.occurred_on,
        row.raw_date_serial,
        row.merchant_text,
        row.actor_text,
        row.category_text,
        row.total_amount,
        row.memo,
        row.unit_price,
        row.quantity,
        row.line_amount,
        row.payment_text,
        row.evidence_text,
        row.extras,
    )
    .fetch_one(&mut *conn)
    .await?;
    Ok(raw_id)
}

/// Insert one row into transactions.
async fn insert_transaction(
    conn: &mut PgConnection,
    owner_id: Uuid,
    t: &TransactionRow,
) -> Result<Uuid> {
    let txn_id: Uuid = sqlx::query_scalar!(
        r#"INSERT INTO transactions (
            owner_id, raw_id, group_id, occurred_on,
            merchant_id, actor_id, category_id, product_id, payment_method_id,
            amount, unit_price, quantity, memo
        ) VALUES (
            $1, $2, $3, $4,
            $5, $6, $7, $8, $9,
            $10, $11, $12, $13
        ) RETURNING id"#,
        owner_id,
        t.raw_id,
        t.group_id,
        t.occurred_on,
        t.merchant_id,
        t.actor_id,
        t.category_id,
        t.product_id,
        t.payment_method_id,
        t.amount,
        t.unit_price,
        t.quantity,
        t.memo,
    )
    .fetch_one(&mut *conn)
    .await?;
    Ok(txn_id)
}

/// Group-sum integrity check (PLAN §1).
/// Returns 0 rows on success; non-zero rows are surfaced as warnings.
pub async fn check_group_integrity(
    conn: &mut PgConnection,
    owner_id: Uuid,
    batch_id: Uuid,
) -> Result<Vec<IntegrityWarning>> {
    let rows = sqlx::query!(
        r#"
        SELECT
            g.group_id,
            g.header_total,
            COALESCE(-SUM(t.amount), 0) AS lines_sum
        FROM (
            SELECT group_id, total_amount AS header_total
            FROM transactions_raw
            WHERE is_group_header = true
              AND owner_id = $1
              AND import_batch_id = $2
        ) g
        LEFT JOIN transactions t ON t.group_id = g.group_id AND t.owner_id = $1
        GROUP BY g.group_id, g.header_total
        HAVING g.header_total <> COALESCE(-SUM(t.amount), 0)
        "#,
        owner_id,
        batch_id,
    )
    .fetch_all(&mut *conn)
    .await?;

    let warnings = rows
        .into_iter()
        .map(|r| IntegrityWarning {
            group_id: r.group_id,
            header_total: r.header_total.unwrap_or(Decimal::ZERO),
            lines_sum: r.lines_sum.unwrap_or(Decimal::ZERO),
        })
        .collect();

    Ok(warnings)
}

/// Full import pipeline: raw insert → normalize → transaction rows → integrity check.
/// All queries run inside the caller-managed transaction (conn).
pub async fn run_pipeline(
    conn: &mut PgConnection,
    owner_id: Uuid,
    batch_id: Uuid,
    rows: Vec<RawRow>,
) -> Result<(i64, Vec<IntegrityWarning>, Vec<UnresolvedAlias>)> {
    let mut transactions_inserted: i64 = 0;
    let mut unresolved: Vec<UnresolvedAlias> = Vec::new();

    for row in &rows {
        // 1. Store raw row.
        let raw_id = insert_raw(conn, owner_id, batch_id, row)
            .await
            .context("Failed to insert raw row")?;

        // 2. Skip rows without a date — can't build a transaction.
        let occurred_on = match row.occurred_on {
            Some(d) => d,
            None => {
                tracing::warn!("Row {}: no date, skipping transaction creation", row.row_index);
                continue;
            }
        };

        // 3. Normalize each text column → entity id.

        // merchant
        let merchant_id = if let Some(ref text) = row.merchant_text {
            let (id, is_new) = upsert_merchant(conn, owner_id, text).await?;
            if is_new {
                unresolved.push(UnresolvedAlias {
                    scope: "merchant".to_string(),
                    raw_text: text.clone(),
                    norm_key: to_norm_key(text),
                });
            }
            Some(id)
        } else {
            None
        };

        // actor
        let actor_id = if let Some(ref text) = row.actor_text {
            let (id, _) = upsert_actor(conn, owner_id, text).await?;
            Some(id)
        } else {
            None
        };

        // category
        let category_id = if let Some(ref text) = row.category_text {
            let (id, is_new) = upsert_category(conn, owner_id, text).await?;
            if is_new {
                unresolved.push(UnresolvedAlias {
                    scope: "category".to_string(),
                    raw_text: text.clone(),
                    norm_key: to_norm_key(text),
                });
            }
            Some(id)
        } else {
            None
        };

        // payment_method
        let payment_method_id = if let Some(ref text) = row.payment_text {
            let (id, _) = upsert_payment_method(conn, owner_id, text).await?;
            Some(id)
        } else {
            None
        };

        // 4. amount 계산 — 엑셀 부호를 반전해서 캐시플로우 부호로 저장.
        //    기존 sign 컬럼은 폐기. 환불(엑셀 음수)은 저장 시 양수가 되어
        //    같은 expense 카테고리 안에서 자연 차감.
        let raw_amount = match row.line_amount.or(row.total_amount) {
            Some(a) => a,
            None => {
                tracing::warn!("Row {}: no amount, skipping", row.row_index);
                continue;
            }
        };
        let amount = -raw_amount;

        // 5. Product mapping (memo-bearing rows only).
        let product_id = if let (Some(ref memo), Some(merch_id)) = (&row.memo, merchant_id) {
            if !memo.is_empty() {
                let (id, is_new) = upsert_product(conn, owner_id, merch_id, memo).await?;
                if is_new {
                    unresolved.push(UnresolvedAlias {
                        scope: "product".to_string(),
                        raw_text: memo.clone(),
                        norm_key: to_norm_key(memo),
                    });
                }
                Some(id)
            } else {
                None
            }
        } else {
            None
        };

        // 6. Insert normalized transaction.
        let t = TransactionRow {
            raw_id,
            group_id: row.group_id,
            occurred_on,
            merchant_id,
            actor_id,
            category_id,
            product_id,
            payment_method_id,
            amount,
            unit_price: row.unit_price,
            quantity: row.quantity,
            memo: row.memo.clone(),
        };

        insert_transaction(conn, owner_id, &t).await?;
        transactions_inserted += 1;
    }

    // 7. Group-sum integrity check (within the same transaction).
    let integrity_warnings = check_group_integrity(conn, owner_id, batch_id).await?;

    if !integrity_warnings.is_empty() {
        tracing::warn!(
            "Group integrity violations: {} groups",
            integrity_warnings.len()
        );
        for w in &integrity_warnings {
            tracing::warn!(
                "  group_id={}, header_total={}, lines_sum={}",
                w.group_id,
                w.header_total,
                w.lines_sum
            );
        }
    }

    Ok((transactions_inserted, integrity_warnings, unresolved))
}
