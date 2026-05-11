/// M2 concurrency tests: verify that concurrent upserts do not produce duplicate rows.
///
/// Strategy: two tokio tasks start simultaneously via a Barrier(2), each acquires its own
/// pool connection, begins a transaction, calls the relevant insert, and commits.
/// Post-condition: exactly 1 row exists for the given key.

mod common;

use finance_manager::entity::{import_batches, prelude::ImportBatches};
use finance_manager::import::pipeline::run_pipeline;
use finance_manager::import::normalize::to_norm_key;
use finance_manager::domain::RawRow;
use sea_orm::{
    ActiveValue::Set, DatabaseBackend, EntityTrait, FromQueryResult, Statement, TransactionTrait,
};
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
async fn insert_batch(db: &sea_orm::DatabaseConnection, owner_id: Uuid, suffix: &str) -> Uuid {
    let hash = format!("fake-hash-{}-{}", owner_id, suffix).into_bytes();
    ImportBatches::insert(import_batches::ActiveModel {
        owner_id: Set(owner_id),
        file_name: Set(format!("test_{}.xlsx", suffix)),
        file_hash: Set(hash),
        year: Set(2026),
        month: Set(1),
        row_count: Set(1),
        ..Default::default()
    })
    .exec(db)
    .await
    .unwrap()
    .last_insert_id
}

// ── Test: concurrent category upsert ─────────────────────────────────────────

/// Two tasks attempt to create the same root category simultaneously.
/// Exactly 1 row must exist after both commit.
#[tokio::test]
async fn concurrent_category_upsert_no_duplicate() {
    let t = common::TestDb::new().await;
    let db = t.db.clone(); // Arc<DatabaseConnection>
    let owner_id = Uuid::new_v4();
    let cat_name = format!("concurrency_test_{}", Uuid::new_v4());

    let barrier = Arc::new(Barrier::new(2));

    let db1 = db.clone();
    let barrier1 = barrier.clone();
    let cat1 = cat_name.clone();
    let oid1 = owner_id;

    let db2 = db.clone();
    let barrier2 = barrier.clone();
    let cat2 = cat_name.clone();
    let oid2 = owner_id;

    let h1 = tokio::spawn(async move {
        let batch_id = insert_batch(&db1, oid1, "c1").await;
        let group_id = Uuid::new_v4();
        let row = make_raw_row(Some(cat1), None, None, group_id);

        // Wait at the barrier so both tasks start the transaction at the same instant.
        barrier1.wait().await; // line: real concurrency starts here

        let tx = db1.begin().await.unwrap();
        run_pipeline(&tx, oid1, batch_id, vec![row]).await.unwrap();
        tx.commit().await.unwrap();
    });

    let h2 = tokio::spawn(async move {
        let batch_id = insert_batch(&db2, oid2, "c2").await;
        let group_id = Uuid::new_v4();
        let row = make_raw_row(Some(cat2), None, None, group_id);

        barrier2.wait().await; // line: real concurrency starts here

        let tx = db2.begin().await.unwrap();
        run_pipeline(&tx, oid2, batch_id, vec![row]).await.unwrap();
        tx.commit().await.unwrap();
    });

    h1.await.unwrap();
    h2.await.unwrap();

    let norm = to_norm_key(&cat_name);

    #[derive(FromQueryResult)]
    struct CountRow { c: i64 }
    let row = CountRow::find_by_statement(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"SELECT COUNT(*)::bigint AS c FROM categories WHERE owner_id = $1 AND name = $2 AND parent_id IS NULL"#,
        [owner_id.into(), norm.into()],
    ))
    .one(&*t.db)
    .await
    .unwrap()
    .unwrap();
    let count = row.c;

    assert_eq!(count, 1, "Expected exactly 1 category row, got {}", count);
}

// ── Test: concurrent product upsert ──────────────────────────────────────────

/// Two tasks attempt to create the same product (same merchant, same memo) simultaneously.
/// Exactly 1 product row must exist after both commit.
#[tokio::test]
async fn concurrent_product_upsert_no_duplicate() {
    let t = common::TestDb::new().await;
    let db = t.db.clone(); // Arc<DatabaseConnection>
    let owner_id = Uuid::new_v4();

    // Use a fresh UUID as part of the name — no underscores so normalization is stable.
    let unique_suffix = Uuid::new_v4().to_string().replace('-', "");
    // All text values use only alphanumerics to avoid normalization surprises.
    let merchant_text = format!("testmerch{}", &unique_suffix[..8]);
    let category_text = format!("testcat{}", &unique_suffix[..8]);
    let memo = format!("testproduct{}", &unique_suffix[..8]);

    let barrier = Arc::new(Barrier::new(2));

    let db1 = db.clone();
    let barrier1 = barrier.clone();
    let memo1 = memo.clone();
    let cat1 = category_text.clone();
    let merch1 = merchant_text.clone();
    let oid1 = owner_id;

    let db2 = db.clone();
    let barrier2 = barrier.clone();
    let memo2 = memo.clone();
    let cat2 = category_text.clone();
    let merch2 = merchant_text.clone();
    let oid2 = owner_id;

    let h1 = tokio::spawn(async move {
        let batch_id = insert_batch(&db1, oid1, "p1").await;
        let group_id = Uuid::new_v4();
        let row = make_raw_row(Some(cat1), Some(merch1), Some(memo1), group_id);

        barrier1.wait().await; // line: real concurrency starts here

        let tx = db1.begin().await.unwrap();
        run_pipeline(&tx, oid1, batch_id, vec![row]).await.unwrap();
        tx.commit().await.unwrap();
    });

    let h2 = tokio::spawn(async move {
        let batch_id = insert_batch(&db2, oid2, "p2").await;
        let group_id = Uuid::new_v4();
        let row = make_raw_row(Some(cat2), Some(merch2), Some(memo2), group_id);

        barrier2.wait().await; // line: real concurrency starts here

        let tx = db2.begin().await.unwrap();
        run_pipeline(&tx, oid2, batch_id, vec![row]).await.unwrap();
        tx.commit().await.unwrap();
    });

    h1.await.unwrap();
    h2.await.unwrap();

    // Verify exactly 1 product row exists (merchant_id resolved via alias table).
    let norm_memo = to_norm_key(&memo);

    #[derive(FromQueryResult)]
    struct CountRow { c: i64 }
    let row = CountRow::find_by_statement(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"SELECT COUNT(*)::bigint AS c FROM products WHERE owner_id = $1 AND name = $2"#,
        [owner_id.into(), norm_memo.into()],
    ))
    .one(&*t.db)
    .await
    .unwrap()
    .unwrap();
    let count = row.c;

    assert_eq!(count, 1, "Expected exactly 1 product row, got {}", count);
}
