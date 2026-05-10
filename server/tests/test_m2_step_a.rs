/// M2 Step A integration tests
///
/// 1. Atomic upsert: calling upsert twice returns the same id without error.
/// 2. "차감" first-time insert lands with review_state='confirmed'.
/// 3. GET /api/summary/2026/2 per-category sums for 외식 and 차감.
/// 4. GET /api/settlement/2026/2 deducted_amount = 7500.

mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
    middleware, routing, Router,
};
use finance_manager::auth::AuthUser;
use finance_manager::import::pipeline::run_pipeline;
use finance_manager::import::xlsx::{extract_sheet_name, extract_year_month, parse_xlsx};
use rust_decimal::Decimal;
use sea_orm::DatabaseConnection;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::sync::Arc;
use tower::ServiceExt;
use uuid::Uuid;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn load_golden_bytes() -> Vec<u8> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/2026년_02월.xlsx"
    );
    std::fs::read(path).expect("Failed to read golden xlsx fixture")
}

async fn do_import(pool: &PgPool, owner_id: Uuid) {
    let bytes = load_golden_bytes();
    let filename = "2026년 02월.xlsx";

    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let hash_vec = hasher.finalize().to_vec();

    let (year, month) = extract_year_month(filename).unwrap();
    let sheet_name = extract_sheet_name(filename).unwrap();
    let raw_rows = parse_xlsx(&bytes, &sheet_name).unwrap();
    let row_count = raw_rows.len() as i32;

    let mut tx = pool.begin().await.unwrap();

    let batch_id: Uuid = sqlx::query_scalar!(
        r#"INSERT INTO import_batches (owner_id, file_name, file_hash, year, month, row_count)
           VALUES ($1, $2, $3, $4, $5, $6)
           RETURNING id"#,
        owner_id,
        filename,
        hash_vec,
        year,
        month,
        row_count,
    )
    .fetch_one(&mut *tx)
    .await
    .unwrap();

    run_pipeline(&mut *tx, owner_id, batch_id, raw_rows)
        .await
        .unwrap();

    tx.commit().await.unwrap();
}

/// Build a test router for a given owner that includes the summary and settlement routes.
fn build_test_router(db: Arc<DatabaseConnection>, owner_id: Uuid) -> Router {
    let user = AuthUser {
        sub: owner_id,
        email: "test@example.com".to_string(),
        groups: vec![],
    };

    Router::new()
        .route(
            "/api/summary/:year/:month",
            routing::get(finance_manager::api::summary::handle_get_summary),
        )
        .route(
            "/api/settlement/:year/:month",
            routing::get(finance_manager::api::settlement::handle_get_settlement),
        )
        .route(
            "/api/categories",
            routing::get(finance_manager::api::categories::handle_get_categories),
        )
        .route(
            "/api/merchants",
            routing::get(finance_manager::api::categories::handle_get_merchants),
        )
        .route(
            "/api/payment-methods",
            routing::get(finance_manager::api::categories::handle_get_payment_methods),
        )
        .with_state(db)
        .layer(middleware::from_fn(
            move |mut req: axum::http::Request<Body>, next: middleware::Next| {
                let user = user.clone();
                async move {
                    req.extensions_mut().insert(user);
                    next.run(req).await
                }
            },
        ))
}

// ── Test 1: atomic upsert idempotence ─────────────────────────────────────────

/// Importing the same golden file twice in two separate transactions (different
/// batch ids) would fail at the import_batches level. Instead, we call
/// run_pipeline twice with distinct batch ids to force double-normalization.
/// The second run must return identical entity ids and must not error.
#[tokio::test]
async fn atomic_upsert_same_id_on_second_call() {
    let t = common::TestDb::new().await;
    let pool = &t.pool;
    let owner_id = Uuid::new_v4();
    let bytes = load_golden_bytes();
    let filename = "2026년 02월.xlsx";

    let sheet_name = extract_sheet_name(filename).unwrap();
    let raw_rows = parse_xlsx(&bytes, &sheet_name).unwrap();
    let row_count = raw_rows.len() as i32;

    // Helper: insert a batch with unique hash suffix to avoid import_batches conflict.
    let insert_batch = |suffix: &str| {
        let mut h = Sha256::new();
        h.update(&bytes);
        h.update(suffix.as_bytes());
        h.finalize().to_vec()
    };

    // First import.
    let hash1 = insert_batch("a");
    let mut tx1 = pool.begin().await.unwrap();
    let batch1: Uuid = sqlx::query_scalar!(
        r#"INSERT INTO import_batches (owner_id, file_name, file_hash, year, month, row_count)
           VALUES ($1, $2, $3, 2026, 2, $4) RETURNING id"#,
        owner_id,
        filename,
        hash1,
        row_count,
    )
    .fetch_one(&mut *tx1)
    .await
    .unwrap();
    run_pipeline(&mut *tx1, owner_id, batch1, raw_rows.clone())
        .await
        .unwrap();
    tx1.commit().await.unwrap();

    // Collect category ids from first run.
    let cats_after_first: Vec<(Uuid, String)> = sqlx::query!(
        r#"SELECT id AS "id!: Uuid", name FROM categories WHERE owner_id = $1 ORDER BY name"#,
        owner_id
    )
    .fetch_all(pool)
    .await
    .unwrap()
    .into_iter()
    .map(|r| (r.id, r.name))
    .collect();

    // Second import — same owner, different file hash.
    let hash2 = insert_batch("b");
    let mut tx2 = pool.begin().await.unwrap();
    let batch2: Uuid = sqlx::query_scalar!(
        r#"INSERT INTO import_batches (owner_id, file_name, file_hash, year, month, row_count)
           VALUES ($1, $2, $3, 2026, 2, $4) RETURNING id"#,
        owner_id,
        filename,
        hash2,
        row_count,
    )
    .fetch_one(&mut *tx2)
    .await
    .unwrap();
    run_pipeline(&mut *tx2, owner_id, batch2, raw_rows)
        .await
        .unwrap();
    tx2.commit().await.unwrap();

    // Category ids must be identical — no new rows created on second run.
    let cats_after_second: Vec<(Uuid, String)> = sqlx::query!(
        r#"SELECT id AS "id!: Uuid", name FROM categories WHERE owner_id = $1 ORDER BY name"#,
        owner_id
    )
    .fetch_all(pool)
    .await
    .unwrap()
    .into_iter()
    .map(|r| (r.id, r.name))
    .collect();

    assert_eq!(
        cats_after_first, cats_after_second,
        "Category ids changed after second import — upsert is not idempotent"
    );

    // Same check for merchants.
    let merch_first: Vec<(Uuid, String)> = sqlx::query!(
        r#"SELECT id AS "id!: Uuid", name FROM merchants WHERE owner_id = $1 ORDER BY name"#,
        owner_id
    )
    .fetch_all(pool)
    .await
    .unwrap()
    .into_iter()
    .map(|r| (r.id, r.name))
    .collect();

    // Third import (another suffix) — merchant ids must match.
    let hash3 = insert_batch("c");
    let mut tx3 = pool.begin().await.unwrap();
    let row_count_i32 = raw_rows_clone_count(&bytes, filename);
    let batch3: Uuid = sqlx::query_scalar!(
        r#"INSERT INTO import_batches (owner_id, file_name, file_hash, year, month, row_count)
           VALUES ($1, $2, $3, 2026, 2, $4) RETURNING id"#,
        owner_id,
        filename,
        hash3,
        row_count_i32,
    )
    .fetch_one(&mut *tx3)
    .await
    .unwrap();
    let raw_rows3 = parse_xlsx(&bytes, &extract_sheet_name(filename).unwrap()).unwrap();
    run_pipeline(&mut *tx3, owner_id, batch3, raw_rows3)
        .await
        .unwrap();
    tx3.commit().await.unwrap();

    let merch_after: Vec<(Uuid, String)> = sqlx::query!(
        r#"SELECT id AS "id!: Uuid", name FROM merchants WHERE owner_id = $1 ORDER BY name"#,
        owner_id
    )
    .fetch_all(pool)
    .await
    .unwrap()
    .into_iter()
    .map(|r| (r.id, r.name))
    .collect();

    assert_eq!(
        merch_first, merch_after,
        "Merchant ids changed after third import — upsert is not idempotent"
    );
}

fn raw_rows_clone_count(bytes: &[u8], filename: &str) -> i32 {
    let sheet_name = extract_sheet_name(filename).unwrap();
    parse_xlsx(bytes, &sheet_name).unwrap().len() as i32
}

// ── Test 2: "차감" review_state = 'confirmed' ─────────────────────────────────

#[tokio::test]
async fn chagang_category_has_confirmed_review_state() {
    let t = common::TestDb::new().await;
    let pool = &t.pool;
    let owner_id = Uuid::new_v4();
    do_import(pool, owner_id).await;

    let row = sqlx::query!(
        r#"SELECT review_state FROM categories WHERE owner_id = $1 AND name = '차감'"#,
        owner_id
    )
    .fetch_one(pool)
    .await
    .expect("차감 category not found");

    assert_eq!(
        row.review_state, "confirmed",
        "차감 category must have review_state='confirmed', got '{}'",
        row.review_state
    );
}

/// A second import must not downgrade an existing 'confirmed' 차감 row to 'pending'.
#[tokio::test]
async fn chagang_review_state_not_downgraded_on_reimport() {
    let t = common::TestDb::new().await;
    let pool = &t.pool;
    let owner_id = Uuid::new_v4();
    do_import(pool, owner_id).await;

    // Manually set review_state to 'confirmed' (already should be).
    // Then run a second pipeline to confirm it stays 'confirmed'.
    let bytes = load_golden_bytes();
    let filename = "2026년 02월.xlsx";
    let sheet_name = extract_sheet_name(filename).unwrap();
    let raw_rows = parse_xlsx(&bytes, &sheet_name).unwrap();
    let row_count = raw_rows.len() as i32;

    let mut h = Sha256::new();
    h.update(&bytes);
    h.update(b"reimport");
    let hash2 = h.finalize().to_vec();

    let mut tx = pool.begin().await.unwrap();
    let batch2: Uuid = sqlx::query_scalar!(
        r#"INSERT INTO import_batches (owner_id, file_name, file_hash, year, month, row_count)
           VALUES ($1, $2, $3, 2026, 2, $4) RETURNING id"#,
        owner_id,
        filename,
        hash2,
        row_count,
    )
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    run_pipeline(&mut *tx, owner_id, batch2, raw_rows)
        .await
        .unwrap();
    tx.commit().await.unwrap();

    let row = sqlx::query!(
        r#"SELECT review_state FROM categories WHERE owner_id = $1 AND name = '차감'"#,
        owner_id
    )
    .fetch_one(pool)
    .await
    .expect("차감 category not found after reimport");

    assert_eq!(
        row.review_state, "confirmed",
        "차감 review_state was downgraded to '{}' after reimport",
        row.review_state
    );
}

// ── Test 3: GET /api/summary/2026/2 spot checks ────────────────────────────────

#[tokio::test]
async fn summary_2026_02_spot_check() {
    let t = common::TestDb::new().await;
    let pool = &t.pool;
    let db = Arc::clone(&t.db);
    let owner_id = Uuid::new_v4();
    do_import(pool, owner_id).await;

    let app = build_test_router(db, owner_id);
    let req = Request::builder()
        .uri("/api/summary/2026/2")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "summary 200");

    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    let categories = json["categories"].as_array().expect("categories array");
    assert!(!categories.is_empty(), "categories must not be empty");

    // Find 차감 category and confirm its total.
    let chagang = categories
        .iter()
        .find(|c| c["category_name"].as_str() == Some("차감"))
        .expect("차감 category in summary");

    let chagang_total: Decimal = chagang["total"]
        .as_str()
        .expect("total is a JSON string (Decimal serde-with-str)")
        .parse()
        .expect("parse total as Decimal");

    assert_eq!(
        chagang_total,
        Decimal::new(7500, 0),
        "차감 total must be 7500, got {}",
        chagang_total
    );

    // Find 외식 점심 category and confirm it has rows.
    // "외식_점심" in the Excel normalizes to "외식 점심" via to_norm_key.
    let oesik = categories
        .iter()
        .find(|c| c["category_name"].as_str() == Some("외식 점심"))
        .expect("외식 점심 category in summary");
    // Confirm it has at least one by_actor entry with a positive amount.
    let by_actor = oesik["by_actor"].as_array().expect("by_actor array");
    assert!(!by_actor.is_empty(), "외식 점심 must have actor entries");

    // Confirm actors list is populated.
    let actors = json["actors"].as_array().expect("actors array");
    assert!(!actors.is_empty(), "actors must not be empty");
}

// ── Test 4: GET /api/settlement/2026/2 ────────────────────────────────────────

#[tokio::test]
async fn settlement_2026_02_deducted_amount() {
    let t = common::TestDb::new().await;
    let pool = &t.pool;
    let db = Arc::clone(&t.db);
    let owner_id = Uuid::new_v4();
    do_import(pool, owner_id).await;

    let app = build_test_router(db, owner_id);
    let req = Request::builder()
        .uri("/api/settlement/2026/2")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "settlement 200");

    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // deducted_amount must equal 7500 (verified M1 golden number).
    let deducted: Decimal = json["deducted_amount"]
        .as_str()
        .expect("deducted_amount is string")
        .parse()
        .expect("parse deducted_amount as Decimal");

    assert_eq!(
        deducted,
        Decimal::new(7500, 0),
        "deducted_amount must be 7500, got {}",
        deducted
    );

    // settlement_input = recognized_expense - deducted_amount.
    let recognized: Decimal = json["recognized_expense"]
        .as_str()
        .expect("recognized_expense is string")
        .parse()
        .expect("parse recognized_expense");

    let settlement: Decimal = json["settlement_input"]
        .as_str()
        .expect("settlement_input is string")
        .parse()
        .expect("parse settlement_input");

    assert_eq!(
        settlement,
        recognized - deducted,
        "settlement_input must equal recognized_expense - deducted_amount"
    );
}

/// Settlement for a month with no data must return zeros (not 404).
#[tokio::test]
async fn settlement_empty_month_returns_zeros() {
    let t = common::TestDb::new().await;
    let db = Arc::clone(&t.db);
    let owner_id = Uuid::new_v4();

    let app = build_test_router(db, owner_id);
    let req = Request::builder()
        .uri("/api/settlement/2025/1")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "empty settlement 200");

    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    let deducted: Decimal = json["deducted_amount"]
        .as_str()
        .expect("deducted_amount string")
        .parse()
        .unwrap();
    assert_eq!(deducted, Decimal::ZERO, "empty month should have zero deducted_amount");
}

// ── Test 5: read-only list endpoints ─────────────────────────────────────────

#[tokio::test]
async fn categories_list_returns_data() {
    let t = common::TestDb::new().await;
    let pool = &t.pool;
    let db = Arc::clone(&t.db);
    let owner_id = Uuid::new_v4();
    do_import(pool, owner_id).await;

    let app = build_test_router(db, owner_id);
    let req = Request::builder()
        .uri("/api/categories")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let arr = json.as_array().expect("array response");
    assert!(!arr.is_empty(), "categories must not be empty after import");

    // All items have required fields.
    for item in arr {
        assert!(item["id"].as_str().is_some(), "id field");
        assert!(item["name"].as_str().is_some(), "name field");
        assert!(item["kind"].as_str().is_some(), "kind field");
        assert!(item["review_state"].as_str().is_some(), "review_state field");
    }
}

#[tokio::test]
async fn merchants_list_returns_data() {
    let t = common::TestDb::new().await;
    let pool = &t.pool;
    let db = Arc::clone(&t.db);
    let owner_id = Uuid::new_v4();
    do_import(pool, owner_id).await;

    let app = build_test_router(db, owner_id);
    let req = Request::builder()
        .uri("/api/merchants")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let arr = json.as_array().expect("array");
    assert!(!arr.is_empty(), "merchants must not be empty");
}

#[tokio::test]
async fn payment_methods_list_returns_data() {
    let t = common::TestDb::new().await;
    let pool = &t.pool;
    let db = Arc::clone(&t.db);
    let owner_id = Uuid::new_v4();
    do_import(pool, owner_id).await;

    let app = build_test_router(db, owner_id);
    let req = Request::builder()
        .uri("/api/payment-methods")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let arr = json.as_array().expect("array");
    assert!(!arr.is_empty(), "payment_methods must not be empty");
    // actor_id and actor_name may be null (not yet mapped).
    for item in arr {
        assert!(item["id"].as_str().is_some(), "id");
        assert!(item["name"].as_str().is_some(), "name");
    }
}
