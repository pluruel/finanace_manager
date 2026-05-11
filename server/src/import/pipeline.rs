use anyhow::{Context, Result};
use rust_decimal::Decimal;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait,
    DatabaseBackend, DatabaseTransaction, EntityTrait, FromQueryResult, QueryFilter,
    Statement, sea_query::OnConflict,
};
use uuid::Uuid;

use crate::domain::{IntegrityWarning, RawRow, TransactionRow, UnresolvedAlias};
use crate::import::normalize::to_norm_key;
use crate::entity::{
    aliases, categories, ledger_actors, merchants, payment_methods, products,
    transactions_raw,
    prelude::*,
};

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
    txn: &DatabaseTransaction,
    owner_id: Uuid,
    raw_text: &str,
) -> Result<(Uuid, bool)> {
    let norm = to_norm_key(raw_text);

    // 1. Alias lookup (cheapest path).
    let existing = Aliases::find()
        .filter(aliases::Column::OwnerId.eq(owner_id))
        .filter(aliases::Column::Scope.eq("category"))
        .filter(aliases::Column::NormKey.eq(&norm))
        .one(txn)
        .await?;

    if let Some(row) = existing {
        return Ok((row.target_id, false));
    }

    // "차감" (normalized) always gets review_state='confirmed' on first creation.
    let is_deduction = norm == "차감";
    let review_state = if is_deduction { "confirmed" } else { "pending" };

    // 카테고리 이름 휴리스틱: 정규화된 이름에 income 키워드 포함 시 'income', 그 외 'expense'.
    // ON CONFLICT DO NOTHING 으로 기존 row 의 kind 는 보존됨 (사용자 토글 / 잘못된 휴리스틱
    // 모두 한 번 결정되면 유지). 휴리스틱은 보조이지 정답이 아님 — 실데이터에서 false positive 가
    // 발견되면 /aliases Categories 탭에서 토글하면 영구 보존됨.
    // "보험" 단독은 보험료(지출)로 두고, 부호 분리된 income 측 카테고리 이름인 "보험금"만 매칭.
    const INCOME_KEYWORDS: &[&str] = &["급여", "수입", "회수", "환급", "보험금"];
    let kind = if INCOME_KEYWORDS.iter().any(|kw| norm.contains(kw)) {
        "income"
    } else {
        "expense"
    };

    // 2. INSERT targeting the partial index; fallback SELECT on conflict. (Pattern B)
    let new_id_opt: Option<Uuid> = txn
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"INSERT INTO categories (owner_id, name, kind, review_state)
               VALUES ($1, $2, $3, $4)
               ON CONFLICT (owner_id, name) WHERE parent_id IS NULL DO NOTHING
               RETURNING id"#,
            [
                owner_id.into(),
                norm.clone().into(),
                kind.into(),
                review_state.into(),
            ],
        ))
        .await
        .context("category INSERT failed")?
        .map(|r| r.try_get::<Uuid>("", "id"))
        .transpose()?;

    let (cat_id, is_new) = match new_id_opt {
        Some(id) => (id, true),
        None => {
            let row = Categories::find()
                .filter(categories::Column::OwnerId.eq(owner_id))
                .filter(categories::Column::Name.eq(&norm))
                .filter(categories::Column::ParentId.is_null())
                .one(txn)
                .await?
                .context("category conflict-fallback SELECT failed")?;
            (row.id, false)
        }
    };

    // 3. Register alias. (Pattern C)
    Aliases::insert(aliases::ActiveModel {
        owner_id: Set(owner_id),
        scope: Set("category".into()),
        raw_text: Set(raw_text.to_string()),
        norm_key: Set(norm),
        target_id: Set(cat_id),
        ..Default::default()
    })
    .on_conflict(
        OnConflict::columns([
            aliases::Column::OwnerId,
            aliases::Column::Scope,
            aliases::Column::NormKey,
        ])
        .do_nothing()
        .to_owned(),
    )
    .exec(txn)
    .await
    .ok(); // ignore RecordNotInserted (conflict = already exists, which is fine)

    Ok((cat_id, is_new))
}

async fn upsert_merchant(
    txn: &DatabaseTransaction,
    owner_id: Uuid,
    raw_text: &str,
) -> Result<(Uuid, bool)> {
    let norm = to_norm_key(raw_text);

    // 1. Alias lookup.
    let existing = Aliases::find()
        .filter(aliases::Column::OwnerId.eq(owner_id))
        .filter(aliases::Column::Scope.eq("merchant"))
        .filter(aliases::Column::NormKey.eq(&norm))
        .one(txn)
        .await?;

    if let Some(row) = existing {
        return Ok((row.target_id, false));
    }

    // 2. Atomic INSERT with fallback SELECT. (Pattern A)
    //    merchants.UNIQUE (owner_id, name) has no NULLs — ON CONFLICT works directly.
    let result = Merchants::insert(merchants::ActiveModel {
        owner_id: Set(owner_id),
        name: Set(norm.clone()),
        review_state: Set("pending".into()),
        ..Default::default()
    })
    .on_conflict(
        OnConflict::columns([
            merchants::Column::OwnerId,
            merchants::Column::Name,
        ])
        .do_nothing()
        .to_owned(),
    )
    .exec(txn)
    .await;

    let (merch_id, is_new) = match result {
        Ok(r) => (r.last_insert_id, true),
        Err(sea_orm::DbErr::RecordNotInserted) => {
            let row = Merchants::find()
                .filter(merchants::Column::OwnerId.eq(owner_id))
                .filter(merchants::Column::Name.eq(&norm))
                .one(txn)
                .await?
                .context("merchant conflict-fallback SELECT failed")?;
            (row.id, false)
        }
        Err(e) => return Err(e.into()),
    };

    // Register alias. (Pattern C)
    Aliases::insert(aliases::ActiveModel {
        owner_id: Set(owner_id),
        scope: Set("merchant".into()),
        raw_text: Set(raw_text.to_string()),
        norm_key: Set(norm),
        target_id: Set(merch_id),
        ..Default::default()
    })
    .on_conflict(
        OnConflict::columns([
            aliases::Column::OwnerId,
            aliases::Column::Scope,
            aliases::Column::NormKey,
        ])
        .do_nothing()
        .to_owned(),
    )
    .exec(txn)
    .await
    .ok();

    Ok((merch_id, is_new))
}

async fn upsert_actor(
    txn: &DatabaseTransaction,
    owner_id: Uuid,
    raw_text: &str,
) -> Result<(Uuid, bool)> {
    let norm = to_norm_key(raw_text);

    let existing = Aliases::find()
        .filter(aliases::Column::OwnerId.eq(owner_id))
        .filter(aliases::Column::Scope.eq("actor"))
        .filter(aliases::Column::NormKey.eq(&norm))
        .one(txn)
        .await?;

    if let Some(row) = existing {
        return Ok((row.target_id, false));
    }

    // ledger_actors.UNIQUE (owner_id, name) has no NULLs — ON CONFLICT works directly. (Pattern A)
    let result = LedgerActors::insert(ledger_actors::ActiveModel {
        owner_id: Set(owner_id),
        name: Set(norm.clone()),
        ..Default::default()
    })
    .on_conflict(
        OnConflict::columns([
            ledger_actors::Column::OwnerId,
            ledger_actors::Column::Name,
        ])
        .do_nothing()
        .to_owned(),
    )
    .exec(txn)
    .await;

    let (actor_id, is_new) = match result {
        Ok(r) => (r.last_insert_id, true),
        Err(sea_orm::DbErr::RecordNotInserted) => {
            let row = LedgerActors::find()
                .filter(ledger_actors::Column::OwnerId.eq(owner_id))
                .filter(ledger_actors::Column::Name.eq(&norm))
                .one(txn)
                .await?
                .context("actor conflict-fallback SELECT failed")?;
            (row.id, false)
        }
        Err(e) => return Err(e.into()),
    };

    Aliases::insert(aliases::ActiveModel {
        owner_id: Set(owner_id),
        scope: Set("actor".into()),
        raw_text: Set(raw_text.to_string()),
        norm_key: Set(norm),
        target_id: Set(actor_id),
        ..Default::default()
    })
    .on_conflict(
        OnConflict::columns([
            aliases::Column::OwnerId,
            aliases::Column::Scope,
            aliases::Column::NormKey,
        ])
        .do_nothing()
        .to_owned(),
    )
    .exec(txn)
    .await
    .ok();

    Ok((actor_id, is_new))
}

async fn upsert_payment_method(
    txn: &DatabaseTransaction,
    owner_id: Uuid,
    raw_text: &str,
) -> Result<(Uuid, bool)> {
    let norm = to_norm_key(raw_text);

    let existing = Aliases::find()
        .filter(aliases::Column::OwnerId.eq(owner_id))
        .filter(aliases::Column::Scope.eq("payment_method"))
        .filter(aliases::Column::NormKey.eq(&norm))
        .one(txn)
        .await?;

    if let Some(row) = existing {
        return Ok((row.target_id, false));
    }

    // payment_methods.UNIQUE (owner_id, name) has no NULLs — ON CONFLICT works directly. (Pattern A)
    let result = PaymentMethods::insert(payment_methods::ActiveModel {
        owner_id: Set(owner_id),
        name: Set(norm.clone()),
        ..Default::default()
    })
    .on_conflict(
        OnConflict::columns([
            payment_methods::Column::OwnerId,
            payment_methods::Column::Name,
        ])
        .do_nothing()
        .to_owned(),
    )
    .exec(txn)
    .await;

    let (pm_id, is_new) = match result {
        Ok(r) => (r.last_insert_id, true),
        Err(sea_orm::DbErr::RecordNotInserted) => {
            let row = PaymentMethods::find()
                .filter(payment_methods::Column::OwnerId.eq(owner_id))
                .filter(payment_methods::Column::Name.eq(&norm))
                .one(txn)
                .await?
                .context("payment_method conflict-fallback SELECT failed")?;
            (row.id, false)
        }
        Err(e) => return Err(e.into()),
    };

    Aliases::insert(aliases::ActiveModel {
        owner_id: Set(owner_id),
        scope: Set("payment_method".into()),
        raw_text: Set(raw_text.to_string()),
        norm_key: Set(norm),
        target_id: Set(pm_id),
        ..Default::default()
    })
    .on_conflict(
        OnConflict::columns([
            aliases::Column::OwnerId,
            aliases::Column::Scope,
            aliases::Column::NormKey,
        ])
        .do_nothing()
        .to_owned(),
    )
    .exec(txn)
    .await
    .ok();

    Ok((pm_id, is_new))
}

/// Product upsert (non-NULL merchant_id path only — callers always pass Some(merch_id)).
///
/// The partial index `products_owner_merchant_name_uniq` on (owner_id, merchant_id, name)
/// WHERE merchant_id IS NOT NULL enables ON CONFLICT without an advisory lock.
async fn upsert_product(
    txn: &DatabaseTransaction,
    owner_id: Uuid,
    merchant_id: Uuid,
    memo: &str,
) -> Result<(Uuid, bool)> {
    let norm = to_norm_key(memo);

    // Product aliases are keyed only on norm_key (not merchant), matching M1 behavior.
    let existing = Aliases::find()
        .filter(aliases::Column::OwnerId.eq(owner_id))
        .filter(aliases::Column::Scope.eq("product"))
        .filter(aliases::Column::NormKey.eq(&norm))
        .one(txn)
        .await?;

    if let Some(row) = existing {
        return Ok((row.target_id, false));
    }

    // INSERT targeting the partial index; fallback SELECT on conflict. (Pattern B)
    let new_id_opt: Option<Uuid> = txn
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"INSERT INTO products (owner_id, merchant_id, name, review_state)
               VALUES ($1, $2, $3, 'pending')
               ON CONFLICT (owner_id, merchant_id, name) WHERE merchant_id IS NOT NULL DO NOTHING
               RETURNING id"#,
            [owner_id.into(), merchant_id.into(), norm.clone().into()],
        ))
        .await?
        .map(|r| r.try_get::<Uuid>("", "id"))
        .transpose()?;

    let (prod_id, is_new) = match new_id_opt {
        Some(id) => (id, true),
        None => {
            let row = Products::find()
                .filter(products::Column::OwnerId.eq(owner_id))
                .filter(products::Column::MerchantId.eq(merchant_id))
                .filter(products::Column::Name.eq(&norm))
                .one(txn)
                .await?
                .context("product conflict-fallback SELECT failed")?;
            (row.id, false)
        }
    };

    Aliases::insert(aliases::ActiveModel {
        owner_id: Set(owner_id),
        scope: Set("product".into()),
        raw_text: Set(memo.to_string()),
        norm_key: Set(norm),
        target_id: Set(prod_id),
        ..Default::default()
    })
    .on_conflict(
        OnConflict::columns([
            aliases::Column::OwnerId,
            aliases::Column::Scope,
            aliases::Column::NormKey,
        ])
        .do_nothing()
        .to_owned(),
    )
    .exec(txn)
    .await
    .ok();

    Ok((prod_id, is_new))
}

/// Insert one row into transactions_raw. (Pattern D)
async fn insert_raw(
    txn: &DatabaseTransaction,
    owner_id: Uuid,
    batch_id: Uuid,
    row: &RawRow,
) -> Result<Uuid> {
    let inserted = transactions_raw::ActiveModel {
        owner_id: Set(owner_id),
        import_batch_id: Set(batch_id),
        row_index: Set(row.row_index),
        group_id: Set(row.group_id),
        is_group_header: Set(row.is_group_header),
        occurred_on: Set(row.occurred_on),
        raw_date_serial: Set(row.raw_date_serial),
        merchant_text: Set(row.merchant_text.clone()),
        actor_text: Set(row.actor_text.clone()),
        category_text: Set(row.category_text.clone()),
        total_amount: Set(row.total_amount),
        memo: Set(row.memo.clone()),
        unit_price: Set(row.unit_price),
        quantity: Set(row.quantity),
        line_amount: Set(row.line_amount),
        payment_text: Set(row.payment_text.clone()),
        evidence_text: Set(row.evidence_text.clone()),
        extras: Set(row.extras.clone()),
        ..Default::default()
    }
    .insert(txn)
    .await?;
    Ok(inserted.id)
}

/// Insert one row into transactions. (Pattern D)
async fn insert_transaction(
    txn: &DatabaseTransaction,
    owner_id: Uuid,
    t: &TransactionRow,
) -> Result<Uuid> {
    use crate::entity::transactions;
    let inserted = transactions::ActiveModel {
        owner_id: Set(owner_id),
        raw_id: Set(t.raw_id),
        group_id: Set(t.group_id),
        occurred_on: Set(t.occurred_on),
        merchant_id: Set(t.merchant_id),
        actor_id: Set(t.actor_id),
        category_id: Set(t.category_id),
        product_id: Set(t.product_id),
        payment_method_id: Set(t.payment_method_id),
        amount: Set(t.amount),
        unit_price: Set(t.unit_price),
        quantity: Set(t.quantity),
        memo: Set(t.memo.clone()),
        ..Default::default()
    }
    .insert(txn)
    .await?;
    Ok(inserted.id)
}

/// Group-sum integrity check (PLAN §1).
/// Returns 0 rows on success; non-zero rows are surfaced as warnings. (Pattern F)
pub async fn check_group_integrity(
    txn: &DatabaseTransaction,
    owner_id: Uuid,
    batch_id: Uuid,
) -> Result<Vec<IntegrityWarning>> {
    #[derive(sea_orm::FromQueryResult)]
    struct IntegrityRow {
        group_id: Uuid,
        header_total: Option<Decimal>,
        lines_sum: Option<Decimal>,
    }

    let rows = IntegrityRow::find_by_statement(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
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
        [owner_id.into(), batch_id.into()],
    ))
    .all(txn)
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
/// All queries run inside the caller-managed transaction (txn).
pub async fn run_pipeline(
    txn: &DatabaseTransaction,
    owner_id: Uuid,
    batch_id: Uuid,
    rows: Vec<RawRow>,
) -> Result<(i64, Vec<IntegrityWarning>, Vec<UnresolvedAlias>)> {
    let mut transactions_inserted: i64 = 0;
    let mut unresolved: Vec<UnresolvedAlias> = Vec::new();

    for row in &rows {
        // 1. Store raw row.
        let raw_id = insert_raw(txn, owner_id, batch_id, row)
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

        // 3. amount 계산 — Excel 은 지출 장부 부호 (지출 양수, 환불 음수).
        //    DB 는 캐시플로우 부호 (유입 양수, 유출 음수). 그래서 부호 반전.
        //    환불은 저장 후 양수가 되어 같은 expense 카테고리 안에서 자연 차감.
        let raw_amount = match row.line_amount.or(row.total_amount) {
            Some(a) => a,
            None => {
                tracing::warn!("Row {}: no amount, skipping", row.row_index);
                continue;
            }
        };
        let amount = -raw_amount;

        // 4. Normalize each text column → entity id.

        // merchant
        let merchant_id = if let Some(ref text) = row.merchant_text {
            let (id, is_new) = upsert_merchant(txn, owner_id, text).await?;
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
            let (id, _) = upsert_actor(txn, owner_id, text).await?;
            Some(id)
        } else {
            None
        };

        // category — 보험 부호 분리 규칙:
        //   Excel 양수("보험" 보험료 지출) → "보험" (kind=expense)
        //   Excel 음수("보험" 환급/보험금 수령) → "보험금" (kind=income, INCOME_KEYWORDS 매칭)
        // 정확히 norm_key=="보험" 인 행에만 적용 — "자동차보험" 같은 명시적 지출 카테고리는 그대로 둔다.
        let category_id = if let Some(ref text) = row.category_text {
            let resolved_text: String = if to_norm_key(text) == "보험" && raw_amount.is_sign_negative() {
                "보험금".to_string()
            } else {
                text.clone()
            };
            let (id, is_new) = upsert_category(txn, owner_id, &resolved_text).await?;
            if is_new {
                unresolved.push(UnresolvedAlias {
                    scope: "category".to_string(),
                    raw_text: resolved_text.clone(),
                    norm_key: to_norm_key(&resolved_text),
                });
            }
            Some(id)
        } else {
            None
        };

        // payment_method
        let payment_method_id = if let Some(ref text) = row.payment_text {
            let (id, _) = upsert_payment_method(txn, owner_id, text).await?;
            Some(id)
        } else {
            None
        };

        // 5. Product mapping (memo-bearing rows only).
        let product_id = if let (Some(ref memo), Some(merch_id)) = (&row.memo, merchant_id) {
            if !memo.is_empty() {
                let (id, is_new) = upsert_product(txn, owner_id, merch_id, memo).await?;
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

        insert_transaction(txn, owner_id, &t).await?;
        transactions_inserted += 1;
    }

    // 7. Group-sum integrity check (within the same transaction).
    let integrity_warnings = check_group_integrity(txn, owner_id, batch_id).await?;

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
