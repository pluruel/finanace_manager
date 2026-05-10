/// M2 concurrency tests: verify that concurrent upserts do not produce duplicate rows.
///
/// Strategy: two tokio tasks start simultaneously via a Barrier(2), each acquires its own
/// pool connection, begins a transaction, calls the relevant insert, and commits.
/// Post-condition: exactly 1 row exists for the given key.

mod common;

use finance_manager::import::pipeline::run_pipeline;
use finance_manager::import::normalize::to_norm_key;
use finance_manager::domain::RawRow;
use sqlx::PgPool;
use std::sync::Arc;
use tokio::sync::Barrier;
use uuid::Uuid;
use chrono::NaiveDate;
use rust_decimal::Decimal;

// ── helpers ──────────────────────────────────────────────────────────────────

/// Build a minimal single-row RawRow with the given category and merchant text.
fn make_raw_row(
    category_text: Option<String>,
    merchant_text: Option<String>,
    memo: Option<String>,
    group_id: Uuid,
) -> RawRow {
    RawRow {
        row_index: 0,
        group_id,
        is_group_header: false,
        occurred_on: Some(NaiveDate::from_ymd_opt(2026, 1, 1).unwrap()),
        raw_date_serial: None,
        merchant_text,
        actor_text: None,
        category_text,
        total_amount: None,
        memo,
        unit_price: None,
        quantity: None,
        line_amount: Some(Decimal::new(1000, 0)),
        payment_text: None,
        evidence_text: None,
        extras: None,
    }
}

/// Insert a throwaway import_batch row and return its id.
async fn insert_batch(pool: &PgPool, owner_id: Uuid, suffix: &str) -> Uuid {
    let hash = format!("fake-hash-{}-{}", owner_id, suffix).into_bytes();
    sqlx::query_scalar!(
        r#"INSERT INTO import_batches (owner_id, file_name, file_hash, year, month, row_count)
           VALUES ($1, $2, $3, 2026, 1, 1)
           RETURNING id"#,
        owner_id,
        format!("test_{}.xlsx", suffix),
        hash,
    )
    .fetch_one(pool)
    .await
    .unwrap()
}

// ── Test: concurrent category upsert ─────────────────────────────────────────

/// Two tasks attempt to create the same root category simultaneously.
/// Exactly 1 row must exist after both commit.
#[tokio::test]
async fn concurrent_category_upsert_no_duplicate() {
    let t = common::TestDb::new().await;
    let pool = Arc::new(t.pool.clone());
    let owner_id = Uuid::new_v4();
    let cat_name = format!("concurrency_test_{}", Uuid::new_v4());

    let barrier = Arc::new(Barrier::new(2));

    let pool1 = pool.clone();
    let barrier1 = barrier.clone();
    let cat1 = cat_name.clone();
    let oid1 = owner_id;

    let pool2 = pool.clone();
    let barrier2 = barrier.clone();
    let cat2 = cat_name.clone();
    let oid2 = owner_id;

    let h1 = tokio::spawn(async move {
        let batch_id = insert_batch(&pool1, oid1, "c1").await;
        let group_id = Uuid::new_v4();
        let row = make_raw_row(Some(cat1), None, None, group_id);

        // Wait at the barrier so both tasks start the transaction at the same instant.
        barrier1.wait().await; // line: real concurrency starts here

        let mut tx = pool1.begin().await.unwrap();
        run_pipeline(&mut *tx, oid1, batch_id, vec![row]).await.unwrap();
        tx.commit().await.unwrap();
    });

    let h2 = tokio::spawn(async move {
        let batch_id = insert_batch(&pool2, oid2, "c2").await;
        let group_id = Uuid::new_v4();
        let row = make_raw_row(Some(cat2), None, None, group_id);

        barrier2.wait().await; // line: real concurrency starts here

        let mut tx = pool2.begin().await.unwrap();
        run_pipeline(&mut *tx, oid2, batch_id, vec![row]).await.unwrap();
        tx.commit().await.unwrap();
    });

    h1.await.unwrap();
    h2.await.unwrap();

    let norm = to_norm_key(&cat_name);
    let count: i64 = sqlx::query_scalar!(
        r#"SELECT COUNT(*) FROM categories WHERE owner_id = $1 AND name = $2 AND parent_id IS NULL"#,
        owner_id,
        norm,
    )
    .fetch_one(&*pool)
    .await
    .unwrap()
    .unwrap_or(0);

    assert_eq!(count, 1, "Expected exactly 1 category row, got {}", count);
}

// ── Test: concurrent product upsert ──────────────────────────────────────────

/// Two tasks attempt to create the same product (same merchant, same memo) simultaneously.
/// Exactly 1 product row must exist after both commit.
#[tokio::test]
async fn concurrent_product_upsert_no_duplicate() {
    let t = common::TestDb::new().await;
    let pool = Arc::new(t.pool.clone());
    let owner_id = Uuid::new_v4();

    // Use a fresh UUID as part of the name — no underscores so normalization is stable.
    let unique_suffix = Uuid::new_v4().to_string().replace('-', "");
    // All text values use only alphanumerics to avoid normalization surprises.
    let merchant_text = format!("testmerch{}", &unique_suffix[..8]);
    let category_text = format!("testcat{}", &unique_suffix[..8]);
    let memo = format!("testproduct{}", &unique_suffix[..8]);

    let barrier = Arc::new(Barrier::new(2));

    let pool1 = pool.clone();
    let barrier1 = barrier.clone();
    let memo1 = memo.clone();
    let cat1 = category_text.clone();
    let merch1 = merchant_text.clone();
    let oid1 = owner_id;

    let pool2 = pool.clone();
    let barrier2 = barrier.clone();
    let memo2 = memo.clone();
    let cat2 = category_text.clone();
    let merch2 = merchant_text.clone();
    let oid2 = owner_id;

    let h1 = tokio::spawn(async move {
        let batch_id = insert_batch(&pool1, oid1, "p1").await;
        let group_id = Uuid::new_v4();
        let row = make_raw_row(Some(cat1), Some(merch1), Some(memo1), group_id);

        barrier1.wait().await; // line: real concurrency starts here

        let mut tx = pool1.begin().await.unwrap();
        run_pipeline(&mut *tx, oid1, batch_id, vec![row]).await.unwrap();
        tx.commit().await.unwrap();
    });

    let h2 = tokio::spawn(async move {
        let batch_id = insert_batch(&pool2, oid2, "p2").await;
        let group_id = Uuid::new_v4();
        let row = make_raw_row(Some(cat2), Some(merch2), Some(memo2), group_id);

        barrier2.wait().await; // line: real concurrency starts here

        let mut tx = pool2.begin().await.unwrap();
        run_pipeline(&mut *tx, oid2, batch_id, vec![row]).await.unwrap();
        tx.commit().await.unwrap();
    });

    h1.await.unwrap();
    h2.await.unwrap();

    // Verify exactly 1 product row exists (merchant_id resolved via alias table).
    let norm_memo = to_norm_key(&memo);
    let count: i64 = sqlx::query_scalar!(
        r#"SELECT COUNT(*) FROM products WHERE owner_id = $1 AND name = $2"#,
        owner_id,
        norm_memo,
    )
    .fetch_one(&*pool)
    .await
    .unwrap()
    .unwrap_or(0);

    assert_eq!(count, 1, "Expected exactly 1 product row, got {}", count);
}
