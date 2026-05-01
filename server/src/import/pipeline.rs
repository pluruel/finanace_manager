use anyhow::{Context, Result};
use rust_decimal::Decimal;
use sqlx::PgConnection;
use uuid::Uuid;

use crate::domain::{IntegrityWarning, RawRow, TransactionRow, UnresolvedAlias};
use crate::import::normalize::to_norm_key;

/// alias 조회 또는 생성: norm_key → target_id
/// 없으면 엔티티를 새로 생성(review_state=pending) + alias 자동 생성
/// 반환: (target_id, is_new)
///
/// P3: 신규 카테고리는 무조건 kind='expense'로 생성.
/// income 여부는 사용자가 /aliases UI에서 확정.
/// 예외: 카테고리명이 정확히 "차감"이면 review_state='confirmed'로 생성
/// (정산 무결성 핵심 카테고리 — 이름 변경 불가, onboarding 시 owner당 1회 시드 예정 [M2]).
async fn upsert_category(
    conn: &mut PgConnection,
    owner_id: Uuid,
    raw_text: &str,
) -> Result<(Uuid, bool)> {
    let norm = to_norm_key(raw_text);

    // 1. alias에서 norm_key 조회
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

    // 2. 같은 norm_key의 카테고리 직접 조회
    let cat_existing = sqlx::query!(
        r#"SELECT id FROM categories WHERE owner_id = $1 AND name = $2"#,
        owner_id,
        norm
    )
    .fetch_optional(&mut *conn)
    .await?;

    let (cat_id, is_new) = if let Some(row) = cat_existing {
        (row.id, false)
    } else {
        // P3: 신규 카테고리는 항상 kind='expense'.
        // P7: "차감"이면 review_state='confirmed' (정산 핵심 카테고리 — 이름 변경 불가).
        let is_deduction_category = raw_text == "차감";
        let review_state = if is_deduction_category { "confirmed" } else { "pending" };

        let new_id: Uuid = sqlx::query_scalar!(
            r#"INSERT INTO categories (owner_id, name, kind, review_state)
               VALUES ($1, $2, 'expense', $3)
               RETURNING id"#,
            owner_id,
            norm,
            review_state,
        )
        .fetch_one(&mut *conn)
        .await?;
        (new_id, true)
    };

    // alias 생성
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

    let merch_existing = sqlx::query!(
        r#"SELECT id FROM merchants WHERE owner_id = $1 AND name = $2"#,
        owner_id,
        norm
    )
    .fetch_optional(&mut *conn)
    .await?;

    let (merch_id, is_new) = if let Some(row) = merch_existing {
        (row.id, false)
    } else {
        let new_id: Uuid = sqlx::query_scalar!(
            r#"INSERT INTO merchants (owner_id, name, review_state)
               VALUES ($1, $2, 'pending')
               RETURNING id"#,
            owner_id,
            norm,
        )
        .fetch_one(&mut *conn)
        .await?;
        (new_id, true)
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

    let actor_existing = sqlx::query!(
        r#"SELECT id FROM ledger_actors WHERE owner_id = $1 AND name = $2"#,
        owner_id,
        norm
    )
    .fetch_optional(&mut *conn)
    .await?;

    let (actor_id, is_new) = if let Some(row) = actor_existing {
        (row.id, false)
    } else {
        let new_id: Uuid = sqlx::query_scalar!(
            r#"INSERT INTO ledger_actors (owner_id, name) VALUES ($1, $2) RETURNING id"#,
            owner_id,
            norm,
        )
        .fetch_one(&mut *conn)
        .await?;
        (new_id, true)
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

    let pm_existing = sqlx::query!(
        r#"SELECT id FROM payment_methods WHERE owner_id = $1 AND name = $2"#,
        owner_id,
        norm
    )
    .fetch_optional(&mut *conn)
    .await?;

    let (pm_id, is_new) = if let Some(row) = pm_existing {
        (row.id, false)
    } else {
        let new_id: Uuid = sqlx::query_scalar!(
            r#"INSERT INTO payment_methods (owner_id, name) VALUES ($1, $2) RETURNING id"#,
            owner_id,
            norm,
        )
        .fetch_one(&mut *conn)
        .await?;
        (new_id, true)
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

/// product 매핑: (merchant_id, norm_key(memo)) 키로 조회/생성
async fn upsert_product(
    conn: &mut PgConnection,
    owner_id: Uuid,
    merchant_id: Uuid,
    memo: &str,
) -> Result<(Uuid, bool)> {
    let norm = to_norm_key(memo);

    // product scope alias: raw_text = memo, norm_key = norm
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

    // 같은 (merchant_id, norm_key)의 product 조회
    let prod_existing = sqlx::query!(
        r#"SELECT id FROM products WHERE owner_id = $1 AND merchant_id = $2 AND name = $3"#,
        owner_id,
        merchant_id,
        norm
    )
    .fetch_optional(&mut *conn)
    .await?;

    let (prod_id, is_new) = if let Some(row) = prod_existing {
        (row.id, false)
    } else {
        let new_id: Uuid = sqlx::query_scalar!(
            r#"INSERT INTO products (owner_id, merchant_id, name, review_state)
               VALUES ($1, $2, $3, 'pending')
               RETURNING id"#,
            owner_id,
            merchant_id,
            norm,
        )
        .fetch_one(&mut *conn)
        .await?;
        (new_id, true)
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

/// transactions_raw에 행 삽입
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

/// transactions에 행 삽입
async fn insert_transaction(
    conn: &mut PgConnection,
    owner_id: Uuid,
    t: &TransactionRow,
) -> Result<Uuid> {
    let txn_id: Uuid = sqlx::query_scalar!(
        r#"INSERT INTO transactions (
            owner_id, raw_id, group_id, occurred_on,
            merchant_id, actor_id, category_id, product_id, payment_method_id,
            amount, sign, unit_price, quantity, memo
        ) VALUES (
            $1, $2, $3, $4,
            $5, $6, $7, $8, $9,
            $10, $11, $12, $13, $14
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
        t.sign as i16,
        t.unit_price,
        t.quantity,
        t.memo,
    )
    .fetch_one(&mut *conn)
    .await?;
    Ok(txn_id)
}

/// 그룹 합계 무결성 검증 SQL (PLAN §1)
/// 결과 0행 = 정상. 불일치 group_id는 경고 반환.
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
            COALESCE(SUM(t.amount * t.sign), 0) AS lines_sum
        FROM (
            SELECT group_id, total_amount AS header_total
            FROM transactions_raw
            WHERE is_group_header = true
              AND owner_id = $1
              AND import_batch_id = $2
        ) g
        LEFT JOIN transactions t ON t.group_id = g.group_id AND t.owner_id = $1
        GROUP BY g.group_id, g.header_total
        HAVING g.header_total <> COALESCE(SUM(t.amount * t.sign), 0)
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

/// 전체 임포트 파이프라인 실행
/// raw 저장 → 정규화 → transaction 생성 → 합계 무결성 검증
/// 모든 쿼리는 단일 트랜잭션 내에서 실행 (conn은 호출자가 관리하는 트랜잭션).
pub async fn run_pipeline(
    conn: &mut PgConnection,
    owner_id: Uuid,
    batch_id: Uuid,
    rows: Vec<RawRow>,
) -> Result<(i64, Vec<IntegrityWarning>, Vec<UnresolvedAlias>)> {
    // group_id별 행 수 집계 (multi-line 판단)
    let mut group_size_map: std::collections::HashMap<Uuid, usize> =
        std::collections::HashMap::new();
    for row in &rows {
        *group_size_map.entry(row.group_id).or_insert(0) += 1;
    }

    let mut transactions_inserted: i64 = 0;
    let mut unresolved: Vec<UnresolvedAlias> = Vec::new();

    for row in &rows {
        // 1. transactions_raw 삽입
        let raw_id = insert_raw(conn, owner_id, batch_id, row)
            .await
            .context("Failed to insert raw row")?;

        // 2. transactions 생성 여부 판단
        // PLAN §3-6: "multi-line 그룹: 헤더는 미생성, 자식 N개만 라인으로 저장"
        // 실측: 헤더도 자체 line_amount를 가지므로, 헤더의 line_amount를 transaction으로 저장함.
        // total_amount(합계)를 중복 저장하지 않는 방식으로 무결성 유지.
        //   - single-line 그룹 헤더: total_amount(=line_amount)로 1 transaction 생성
        //   - multi-line 그룹 헤더: line_amount로 1 transaction 생성 (total_amount는 무시)
        //   - multi-line 그룹 자식: line_amount로 1 transaction 생성

        // occurred_on 없으면 transaction 생성 불가
        let occurred_on = match row.occurred_on {
            Some(d) => d,
            None => {
                tracing::warn!("Row {}: no date, skipping transaction creation", row.row_index);
                continue;
            }
        };

        // 3. 정규화: 각 텍스트 컬럼 → entity_id

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
        // P3: 신규 카테고리는 항상 kind='expense'. income 여부는 사용자가 UI에서 확정.
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

        // 4. amount, sign 결정
        // amount: 항상 양수 (line_amount의 절댓값)
        // sign: +1=지출, -1=수입(회수)
        // 카테고리="차감"은 sign=+1 (PLAN §6, 영수증 합계 무결성 유지)
        //
        // 모든 경우에 line_amount를 사용:
        // - multi-line 헤더: line_amount (자신의 아이템 금액)
        // - multi-line 자식: line_amount
        // - single-line 헤더: line_amount (= total_amount와 동일)
        let raw_amount = row.line_amount.or(row.total_amount);

        let raw_amount = match raw_amount {
            Some(a) => a,
            None => {
                tracing::warn!("Row {}: no amount, skipping", row.row_index);
                continue;
            }
        };

        // sign 결정: 음수 금액이면 sign=-1 (수입/회수)
        // 카테고리="차감"은 sign=+1 (PLAN §6)
        let is_deduction = row
            .category_text
            .as_deref()
            .map(|c| c == "차감")
            .unwrap_or(false);

        let sign: i16 = if is_deduction {
            1 // 차감은 항상 +1
        } else if raw_amount < Decimal::ZERO {
            -1 // 음수 금액 = 수입/회수
        } else {
            1 // 양수 = 지출
        };

        let amount = raw_amount.abs();

        // 5. product 매핑 (메모 있는 행만)
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

        // 6. transaction 삽입
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
            sign,
            unit_price: row.unit_price,
            quantity: row.quantity,
            memo: row.memo.clone(),
        };

        insert_transaction(conn, owner_id, &t).await?;
        transactions_inserted += 1;
    }

    // 7. 합계 무결성 검증 (같은 트랜잭션 내)
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
