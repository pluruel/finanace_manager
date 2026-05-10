/// M2 Step B integration tests
///
/// 1. merge_merchant_remaps_transactions
/// 2. merge_product_remaps_product_id
/// 3. confirm_rejects_deduction_category
/// 4. concurrent_merges_one_winner
/// 5. payment_method_cross_actor_merge_rejected
/// 6. settlement_unchanged_after_merge

use axum::{
    body::Body,
    http::{Request, StatusCode},
    middleware, routing, Router,
};
use finance_manager::auth::AuthUser;
use finance_manager::import::normalize::to_norm_key;
use finance_manager::import::pipeline::run_pipeline;
use finance_manager::import::xlsx::{extract_sheet_name, extract_year_month, parse_xlsx};
use rust_decimal::Decimal;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::sync::Arc;
use tokio::sync::Barrier;
use tower::ServiceExt;
use uuid::Uuid;

// ── Shared helpers ────────────────────────────────────────────────────────────

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
           VALUES ($1, $2, $3, $4, $5, $6) RETURNING id"#,
        owner_id, filename, hash_vec, year, month, row_count,
    )
    .fetch_one(&mut *tx)
    .await
    .unwrap();

    run_pipeline(&mut *tx, owner_id, batch_id, raw_rows)
        .await
        .unwrap();
    tx.commit().await.unwrap();
}

/// Insert a throwaway import_batch row and return its id.
async fn insert_batch(pool: &PgPool, owner_id: Uuid, suffix: &str) -> Uuid {
    let hash = format!("fake-hash-{}-{}", owner_id, suffix).into_bytes();
    sqlx::query_scalar!(
        r#"INSERT INTO import_batches (owner_id, file_name, file_hash, year, month, row_count)
           VALUES ($1, $2, $3, 2026, 1, 1) RETURNING id"#,
        owner_id,
        format!("test_{}.xlsx", suffix),
        hash,
    )
    .fetch_one(pool)
    .await
    .unwrap()
}

/// Build a test router that wires the aliases + settlement + categories endpoints
/// with a synthetic authenticated user.
fn build_test_router(pool: Arc<PgPool>, owner_id: Uuid) -> Router {
    let user = AuthUser {
        sub: owner_id,
        email: "test@example.com".to_string(),
        groups: vec![],
    };

    Router::new()
        .route(
            "/api/aliases",
            routing::post(finance_manager::api::aliases::handle_post_alias),
        )
        .route(
            "/api/aliases/:id",
            routing::delete(finance_manager::api::aliases::handle_delete_alias),
        )
        .route(
            "/api/review-queue",
            routing::get(finance_manager::api::aliases::handle_get_review_queue),
        )
        .route(
            "/api/entities/:scope/:id/confirm",
            routing::post(finance_manager::api::aliases::handle_confirm_entity),
        )
        .route(
            "/api/settlement/:year/:month",
            routing::get(finance_manager::api::settlement::handle_get_settlement),
        )
        .with_state(pool)
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

// ── Test 1: merge_merchant_remaps_transactions ────────────────────────────────

/// Seed two merchants. Transactions reference the source. After merge via
/// POST /api/aliases, all transactions must point to the target, and the
/// source merchant must be deleted (orphan_deleted=true).
#[sqlx::test(migrations = "./migrations")]
async fn merge_merchant_remaps_transactions(pool: PgPool) {
    let pool = Arc::new(pool);
    let owner_id = Uuid::new_v4();

    // Seed source merchant "이 마트" (with space) and target "이마트" (no space).
    let src_merchant_id: Uuid = sqlx::query_scalar!(
        r#"INSERT INTO merchants (owner_id, name, review_state) VALUES ($1, '이 마트', 'pending') RETURNING id"#,
        owner_id
    )
    .fetch_one(&*pool)
    .await
    .unwrap();

    let tgt_merchant_id: Uuid = sqlx::query_scalar!(
        r#"INSERT INTO merchants (owner_id, name, review_state) VALUES ($1, '이마트', 'confirmed') RETURNING id"#,
        owner_id
    )
    .fetch_one(&*pool)
    .await
    .unwrap();

    // Seed a category and actor for FK satisfaction.
    let cat_id: Uuid = sqlx::query_scalar!(
        r#"INSERT INTO categories (owner_id, name, kind, review_state) VALUES ($1, '식료품', 'expense', 'pending') RETURNING id"#,
        owner_id
    )
    .fetch_one(&*pool)
    .await
    .unwrap();

    let actor_id: Uuid = sqlx::query_scalar!(
        r#"INSERT INTO ledger_actors (owner_id, name) VALUES ($1, '공동') RETURNING id"#,
        owner_id
    )
    .fetch_one(&*pool)
    .await
    .unwrap();

    let pm_id: Uuid = sqlx::query_scalar!(
        r#"INSERT INTO payment_methods (owner_id, name) VALUES ($1, '신한') RETURNING id"#,
        owner_id
    )
    .fetch_one(&*pool)
    .await
    .unwrap();

    let batch_id = insert_batch(&pool, owner_id, "t1").await;

    // Seed 3 transactions referencing the source merchant.
    for i in 0..3_i32 {
        let raw_id: Uuid = sqlx::query_scalar!(
            r#"INSERT INTO transactions_raw
               (owner_id, import_batch_id, row_index, group_id, is_group_header,
                occurred_on, merchant_text, line_amount)
               VALUES ($1, $2, $3, gen_random_uuid(), false, '2026-02-01', '이 마트', 10000)
               RETURNING id"#,
            owner_id, batch_id, i
        )
        .fetch_one(&*pool)
        .await
        .unwrap();

        sqlx::query!(
            r#"INSERT INTO transactions
               (owner_id, raw_id, group_id, occurred_on, merchant_id, actor_id,
                category_id, payment_method_id, amount)
               VALUES ($1, $2, gen_random_uuid(), '2026-02-01', $3, $4, $5, $6, -10000)"#,
            owner_id, raw_id, src_merchant_id, actor_id, cat_id, pm_id
        )
        .execute(&*pool)
        .await
        .unwrap();
    }

    // Register alias for the source merchant (norm_key of "이 마트").
    let norm_src = to_norm_key("이 마트");
    sqlx::query!(
        r#"INSERT INTO aliases (owner_id, scope, raw_text, norm_key, target_id)
           VALUES ($1, 'merchant', '이 마트', $2, $3)"#,
        owner_id, norm_src, src_merchant_id
    )
    .execute(&*pool)
    .await
    .unwrap();

    // POST /api/aliases to merge.
    let app = build_test_router(pool.clone(), owner_id);
    let body = serde_json::json!({
        "scope": "merchant",
        "raw_text": "이 마트",
        "target_id": tgt_merchant_id
    });
    let req = Request::builder()
        .method("POST")
        .uri("/api/aliases")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "POST /api/aliases should be 200");

    let resp_body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&resp_body).unwrap();
    assert_eq!(json["remapped_transaction_count"], 3, "3 transactions should be remapped");
    assert_eq!(json["orphan_deleted"], true, "orphan merchant should be deleted");

    // Verify transactions now reference target.
    let remaining_src: i64 = sqlx::query_scalar!(
        r#"SELECT COUNT(*) FROM transactions WHERE owner_id = $1 AND merchant_id = $2"#,
        owner_id, src_merchant_id
    )
    .fetch_one(&*pool)
    .await
    .unwrap()
    .unwrap_or(0);
    assert_eq!(remaining_src, 0, "no transactions should reference the source merchant");

    let tgt_count: i64 = sqlx::query_scalar!(
        r#"SELECT COUNT(*) FROM transactions WHERE owner_id = $1 AND merchant_id = $2"#,
        owner_id, tgt_merchant_id
    )
    .fetch_one(&*pool)
    .await
    .unwrap()
    .unwrap_or(0);
    assert_eq!(tgt_count, 3, "all 3 transactions should reference the target merchant");

    // Verify source merchant row is gone.
    let src_exists: bool = sqlx::query_scalar!(
        r#"SELECT EXISTS(SELECT 1 FROM merchants WHERE id = $1) AS "e!: bool""#,
        src_merchant_id
    )
    .fetch_one(&*pool)
    .await
    .unwrap();
    assert!(!src_exists, "source merchant row should have been deleted");
}

// ── Test 2: merge_product_remaps_product_id ───────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn merge_product_remaps_product_id(pool: PgPool) {
    let pool = Arc::new(pool);
    let owner_id = Uuid::new_v4();

    // Seed merchant for FK.
    let merchant_id: Uuid = sqlx::query_scalar!(
        r#"INSERT INTO merchants (owner_id, name, review_state) VALUES ($1, '와인숍', 'confirmed') RETURNING id"#,
        owner_id
    )
    .fetch_one(&*pool)
    .await
    .unwrap();

    // Source product "조닌끼안티" and target "조닌 끼안티" (one space difference).
    let src_product_id: Uuid = sqlx::query_scalar!(
        r#"INSERT INTO products (owner_id, merchant_id, name, review_state)
           VALUES ($1, $2, '조닌끼안티', 'pending') RETURNING id"#,
        owner_id, merchant_id
    )
    .fetch_one(&*pool)
    .await
    .unwrap();

    let tgt_product_id: Uuid = sqlx::query_scalar!(
        r#"INSERT INTO products (owner_id, merchant_id, name, review_state)
           VALUES ($1, $2, '조닌 끼안티', 'confirmed') RETURNING id"#,
        owner_id, merchant_id
    )
    .fetch_one(&*pool)
    .await
    .unwrap();

    let cat_id: Uuid = sqlx::query_scalar!(
        r#"INSERT INTO categories (owner_id, name, kind, review_state) VALUES ($1, '와인', 'expense', 'pending') RETURNING id"#,
        owner_id
    )
    .fetch_one(&*pool)
    .await
    .unwrap();

    let actor_id: Uuid = sqlx::query_scalar!(
        r#"INSERT INTO ledger_actors (owner_id, name) VALUES ($1, '공동') RETURNING id"#,
        owner_id
    )
    .fetch_one(&*pool)
    .await
    .unwrap();

    let pm_id: Uuid = sqlx::query_scalar!(
        r#"INSERT INTO payment_methods (owner_id, name) VALUES ($1, '하나') RETURNING id"#,
        owner_id
    )
    .fetch_one(&*pool)
    .await
    .unwrap();

    let batch_id = insert_batch(&pool, owner_id, "p2").await;

    // 2 transactions referencing the source product.
    for i in 0..2_i32 {
        let raw_id: Uuid = sqlx::query_scalar!(
            r#"INSERT INTO transactions_raw
               (owner_id, import_batch_id, row_index, group_id, is_group_header,
                occurred_on, memo, line_amount)
               VALUES ($1, $2, $3, gen_random_uuid(), false, '2026-02-01', '조닌끼안티', 34000)
               RETURNING id"#,
            owner_id, batch_id, i
        )
        .fetch_one(&*pool)
        .await
        .unwrap();

        sqlx::query!(
            r#"INSERT INTO transactions
               (owner_id, raw_id, group_id, occurred_on, merchant_id, actor_id,
                category_id, product_id, payment_method_id, amount, memo)
               VALUES ($1, $2, gen_random_uuid(), '2026-02-01', $3, $4, $5, $6, $7, -34000, '조닌끼안티')"#,
            owner_id, raw_id, merchant_id, actor_id, cat_id, src_product_id, pm_id
        )
        .execute(&*pool)
        .await
        .unwrap();
    }

    // Register alias for the source product.
    let norm_src = to_norm_key("조닌끼안티");
    sqlx::query!(
        r#"INSERT INTO aliases (owner_id, scope, raw_text, norm_key, target_id)
           VALUES ($1, 'product', '조닌끼안티', $2, $3)"#,
        owner_id, norm_src, src_product_id
    )
    .execute(&*pool)
    .await
    .unwrap();

    // POST /api/aliases to merge.
    let app = build_test_router(pool.clone(), owner_id);
    let body = serde_json::json!({
        "scope": "product",
        "raw_text": "조닌끼안티",
        "target_id": tgt_product_id
    });
    let req = Request::builder()
        .method("POST")
        .uri("/api/aliases")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let resp_body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&resp_body).unwrap();
    assert_eq!(json["remapped_transaction_count"], 2);
    assert_eq!(json["orphan_deleted"], true);

    // Verify transactions reference target product.
    let tgt_count: i64 = sqlx::query_scalar!(
        r#"SELECT COUNT(*) FROM transactions WHERE owner_id = $1 AND product_id = $2"#,
        owner_id, tgt_product_id
    )
    .fetch_one(&*pool)
    .await
    .unwrap()
    .unwrap_or(0);
    assert_eq!(tgt_count, 2);

    // Source product row deleted.
    let src_exists: bool = sqlx::query_scalar!(
        r#"SELECT EXISTS(SELECT 1 FROM products WHERE id = $1) AS "e!: bool""#,
        src_product_id
    )
    .fetch_one(&*pool)
    .await
    .unwrap();
    assert!(!src_exists, "source product should be deleted");
}

// ── Test 3: confirm_rejects_deduction_category ────────────────────────────────

/// The "차감" category must be rejected by POST /api/entities/category/:id/confirm.
#[sqlx::test(migrations = "./migrations")]
async fn confirm_rejects_deduction_category(pool: PgPool) {
    let pool = Arc::new(pool);
    let owner_id = Uuid::new_v4();
    do_import(&pool, owner_id).await;

    // Find the 차감 category id.
    let chagang_id: Uuid = sqlx::query_scalar!(
        r#"SELECT id FROM categories WHERE owner_id = $1 AND name = '차감'"#,
        owner_id
    )
    .fetch_one(&*pool)
    .await
    .unwrap();

    let app = build_test_router(pool.clone(), owner_id);
    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/entities/category/{}/confirm", chagang_id))
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::CONFLICT,
        "confirming 차감 category must return 409"
    );
}

// ── Test 4: concurrent_merges_one_winner ──────────────────────────────────────

/// Two tokio tasks attempt to merge the same source merchant into two different
/// targets simultaneously. Exactly one must succeed (200) and the other must 409.
#[sqlx::test(migrations = "./migrations")]
async fn concurrent_merges_one_winner(pool: PgPool) {
    let pool = Arc::new(pool);
    let owner_id = Uuid::new_v4();

    // Seed: source + two targets + supporting entities.
    let src_id: Uuid = sqlx::query_scalar!(
        r#"INSERT INTO merchants (owner_id, name, review_state) VALUES ($1, '소스가맹', 'pending') RETURNING id"#,
        owner_id
    )
    .fetch_one(&*pool)
    .await
    .unwrap();

    let tgt_a_id: Uuid = sqlx::query_scalar!(
        r#"INSERT INTO merchants (owner_id, name, review_state) VALUES ($1, '타겟A', 'confirmed') RETURNING id"#,
        owner_id
    )
    .fetch_one(&*pool)
    .await
    .unwrap();

    let tgt_b_id: Uuid = sqlx::query_scalar!(
        r#"INSERT INTO merchants (owner_id, name, review_state) VALUES ($1, '타겟B', 'confirmed') RETURNING id"#,
        owner_id
    )
    .fetch_one(&*pool)
    .await
    .unwrap();

    // Register alias pointing to the source.
    let norm = to_norm_key("소스가맹");
    sqlx::query!(
        r#"INSERT INTO aliases (owner_id, scope, raw_text, norm_key, target_id)
           VALUES ($1, 'merchant', '소스가맹', $2, $3)"#,
        owner_id, norm, src_id
    )
    .execute(&*pool)
    .await
    .unwrap();

    let barrier = Arc::new(Barrier::new(2));

    let pool1 = pool.clone();
    let barrier1 = barrier.clone();
    let oid1 = owner_id;

    let pool2 = pool.clone();
    let barrier2 = barrier.clone();
    let oid2 = owner_id;

    // Task 1: merge source → target A.
    let h1 = tokio::spawn(async move {
        let user = AuthUser {
            sub: oid1,
            email: "t@t.com".to_string(),
            groups: vec![],
        };
        let app = Router::new()
            .route(
                "/api/aliases",
                routing::post(finance_manager::api::aliases::handle_post_alias),
            )
            .with_state(pool1.clone())
            .layer(middleware::from_fn(
                move |mut req: axum::http::Request<Body>, next: middleware::Next| {
                    let u = user.clone();
                    async move {
                        req.extensions_mut().insert(u);
                        next.run(req).await
                    }
                },
            ));

        barrier1.wait().await;

        let body = serde_json::json!({
            "scope": "merchant",
            "raw_text": "소스가맹",
            "target_id": tgt_a_id,
            "source_id": src_id
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/aliases")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        app.oneshot(req).await.unwrap().status()
    });

    // Task 2: merge source → target B.
    let h2 = tokio::spawn(async move {
        let user = AuthUser {
            sub: oid2,
            email: "t@t.com".to_string(),
            groups: vec![],
        };
        let app = Router::new()
            .route(
                "/api/aliases",
                routing::post(finance_manager::api::aliases::handle_post_alias),
            )
            .with_state(pool2.clone())
            .layer(middleware::from_fn(
                move |mut req: axum::http::Request<Body>, next: middleware::Next| {
                    let u = user.clone();
                    async move {
                        req.extensions_mut().insert(u);
                        next.run(req).await
                    }
                },
            ));

        barrier2.wait().await;

        let body = serde_json::json!({
            "scope": "merchant",
            "raw_text": "소스가맹",
            "target_id": tgt_b_id,
            "source_id": src_id
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/aliases")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        app.oneshot(req).await.unwrap().status()
    });

    let s1 = h1.await.unwrap();
    let s2 = h2.await.unwrap();

    // Exactly one must succeed (200) and the other must 409.
    let ok_count = [s1, s2].iter().filter(|&&s| s == StatusCode::OK).count();
    let conflict_count = [s1, s2]
        .iter()
        .filter(|&&s| s == StatusCode::CONFLICT)
        .count();
    assert_eq!(ok_count, 1, "exactly one merge should succeed; got s1={} s2={}", s1, s2);
    assert_eq!(conflict_count, 1, "the other merge should 409; got s1={} s2={}", s1, s2);
}

// ── Test 5: payment_method_cross_actor_merge_rejected ─────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn payment_method_cross_actor_merge_rejected(pool: PgPool) {
    let pool = Arc::new(pool);
    let owner_id = Uuid::new_v4();

    // Seed two actors.
    let eongnea_id: Uuid = sqlx::query_scalar!(
        r#"INSERT INTO ledger_actors (owner_id, name) VALUES ($1, '엉아') RETURNING id"#,
        owner_id
    )
    .fetch_one(&*pool)
    .await
    .unwrap();

    let baby_id: Uuid = sqlx::query_scalar!(
        r#"INSERT INTO ledger_actors (owner_id, name) VALUES ($1, '아기') RETURNING id"#,
        owner_id
    )
    .fetch_one(&*pool)
    .await
    .unwrap();

    // Source pinned to 엉아, target pinned to 아기.
    let src_pm_id: Uuid = sqlx::query_scalar!(
        r#"INSERT INTO payment_methods (owner_id, name, actor_id) VALUES ($1, '신한엉아', $2) RETURNING id"#,
        owner_id, eongnea_id
    )
    .fetch_one(&*pool)
    .await
    .unwrap();

    let tgt_pm_id: Uuid = sqlx::query_scalar!(
        r#"INSERT INTO payment_methods (owner_id, name, actor_id) VALUES ($1, '신한아기', $2) RETURNING id"#,
        owner_id, baby_id
    )
    .fetch_one(&*pool)
    .await
    .unwrap();

    // Register alias for the source.
    let norm = to_norm_key("신한엉아");
    sqlx::query!(
        r#"INSERT INTO aliases (owner_id, scope, raw_text, norm_key, target_id)
           VALUES ($1, 'payment_method', '신한엉아', $2, $3)"#,
        owner_id, norm, src_pm_id
    )
    .execute(&*pool)
    .await
    .unwrap();

    let app = build_test_router(pool.clone(), owner_id);
    let body = serde_json::json!({
        "scope": "payment_method",
        "raw_text": "신한엉아",
        "target_id": tgt_pm_id
    });
    let req = Request::builder()
        .method("POST")
        .uri("/api/aliases")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::CONFLICT,
        "cross-actor payment method merge must return 409"
    );

    let resp_body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&resp_body).unwrap();
    // Structured error: code must be "actor_mismatch", actors named in dedicated fields.
    assert_eq!(
        json["error"].as_str(),
        Some("actor_mismatch"),
        "error code must be actor_mismatch, got: {}",
        json
    );
    assert_eq!(
        json["source_actor"].as_str(),
        Some("엉아"),
        "source_actor must be '엉아', got: {}",
        json
    );
    assert_eq!(
        json["target_actor"].as_str(),
        Some("아기"),
        "target_actor must be '아기', got: {}",
        json
    );
}

// ── Test 5b: payment_method_same_actor_merge_allowed ─────────────────────────

/// Merging two payment methods pinned to the same actor must succeed.
#[sqlx::test(migrations = "./migrations")]
async fn payment_method_same_actor_merge_allowed(pool: PgPool) {
    let pool = Arc::new(pool);
    let owner_id = Uuid::new_v4();

    let actor_id: Uuid = sqlx::query_scalar!(
        r#"INSERT INTO ledger_actors (owner_id, name) VALUES ($1, '아기') RETURNING id"#,
        owner_id
    )
    .fetch_one(&*pool)
    .await
    .unwrap();

    let src_pm_id: Uuid = sqlx::query_scalar!(
        r#"INSERT INTO payment_methods (owner_id, name, actor_id) VALUES ($1, '롯데A', $2) RETURNING id"#,
        owner_id, actor_id
    )
    .fetch_one(&*pool)
    .await
    .unwrap();

    let tgt_pm_id: Uuid = sqlx::query_scalar!(
        r#"INSERT INTO payment_methods (owner_id, name, actor_id) VALUES ($1, '롯데B', $2) RETURNING id"#,
        owner_id, actor_id
    )
    .fetch_one(&*pool)
    .await
    .unwrap();

    let norm = to_norm_key("롯데A");
    sqlx::query!(
        r#"INSERT INTO aliases (owner_id, scope, raw_text, norm_key, target_id)
           VALUES ($1, 'payment_method', '롯데A', $2, $3)"#,
        owner_id, norm, src_pm_id
    )
    .execute(&*pool)
    .await
    .unwrap();

    let app = build_test_router(pool.clone(), owner_id);
    let body = serde_json::json!({
        "scope": "payment_method",
        "raw_text": "롯데A",
        "target_id": tgt_pm_id
    });
    let req = Request::builder()
        .method("POST")
        .uri("/api/aliases")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "same-actor merge should succeed");
}

// ── Test 5c: payment_method_null_actor_merge_allowed ─────────────────────────

/// Merging where one side has NULL actor_id must be allowed.
#[sqlx::test(migrations = "./migrations")]
async fn payment_method_null_actor_merge_allowed(pool: PgPool) {
    let pool = Arc::new(pool);
    let owner_id = Uuid::new_v4();

    let actor_id: Uuid = sqlx::query_scalar!(
        r#"INSERT INTO ledger_actors (owner_id, name) VALUES ($1, '엉아') RETURNING id"#,
        owner_id
    )
    .fetch_one(&*pool)
    .await
    .unwrap();

    // Source has NULL actor_id, target has a real actor.
    let src_pm_id: Uuid = sqlx::query_scalar!(
        r#"INSERT INTO payment_methods (owner_id, name) VALUES ($1, '현금X') RETURNING id"#,
        owner_id
    )
    .fetch_one(&*pool)
    .await
    .unwrap();

    let tgt_pm_id: Uuid = sqlx::query_scalar!(
        r#"INSERT INTO payment_methods (owner_id, name, actor_id) VALUES ($1, '현금Y', $2) RETURNING id"#,
        owner_id, actor_id
    )
    .fetch_one(&*pool)
    .await
    .unwrap();

    let norm = to_norm_key("현금X");
    sqlx::query!(
        r#"INSERT INTO aliases (owner_id, scope, raw_text, norm_key, target_id)
           VALUES ($1, 'payment_method', '현금X', $2, $3)"#,
        owner_id, norm, src_pm_id
    )
    .execute(&*pool)
    .await
    .unwrap();

    let app = build_test_router(pool.clone(), owner_id);
    let body = serde_json::json!({
        "scope": "payment_method",
        "raw_text": "현금X",
        "target_id": tgt_pm_id
    });
    let req = Request::builder()
        .method("POST")
        .uri("/api/aliases")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "null-actor merge should be allowed");
}

// ── Test 6: settlement_unchanged_after_merge ──────────────────────────────────

/// Import the golden file, then merge two arbitrary merchants. Verify that
/// v_monthly_settlement's deducted_amount and settlement_input remain correct.
#[sqlx::test(migrations = "./migrations")]
async fn settlement_unchanged_after_merge(pool: PgPool) {
    let pool = Arc::new(pool);
    let owner_id = Uuid::new_v4();
    do_import(&pool, owner_id).await;

    // Pick two merchants to merge (any two — just not source=target).
    let merchants: Vec<(Uuid, String)> = sqlx::query!(
        r#"SELECT id AS "id!: Uuid", name AS "name!" FROM merchants WHERE owner_id = $1 ORDER BY name LIMIT 2"#,
        owner_id
    )
    .fetch_all(&*pool)
    .await
    .unwrap()
    .into_iter()
    .map(|r| (r.id, r.name))
    .collect();

    assert!(
        merchants.len() >= 2,
        "Need at least 2 merchants to test merge; got {}",
        merchants.len()
    );

    let (src_id, src_name) = &merchants[0];
    let (tgt_id, _) = &merchants[1];

    // Only merge if there's an alias for the source (pipeline creates them).
    let src_norm = to_norm_key(src_name);
    let alias_exists: bool = sqlx::query_scalar!(
        r#"SELECT EXISTS(SELECT 1 FROM aliases WHERE owner_id = $1 AND scope = 'merchant' AND norm_key = $2) AS "e!: bool""#,
        owner_id, src_norm
    )
    .fetch_one(&*pool)
    .await
    .unwrap();

    if !alias_exists {
        // No alias to merge — skip by inserting one.
        sqlx::query!(
            r#"INSERT INTO aliases (owner_id, scope, raw_text, norm_key, target_id)
               VALUES ($1, 'merchant', $2, $3, $4)
               ON CONFLICT (owner_id, scope, norm_key) DO NOTHING"#,
            owner_id, src_name.as_str(), src_norm, src_id
        )
        .execute(&*pool)
        .await
        .unwrap();
    }

    // Do the merge.
    let app = build_test_router(pool.clone(), owner_id);
    let body = serde_json::json!({
        "scope": "merchant",
        "raw_text": src_name,
        "target_id": tgt_id
    });
    let req = Request::builder()
        .method("POST")
        .uri("/api/aliases")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    // 200 or 200 no-op (if they happened to have same target already).
    assert!(
        resp.status().is_success(),
        "merge should succeed, got {}",
        resp.status()
    );

    // Check settlement numbers.
    let req2 = Request::builder()
        .uri("/api/settlement/2026/2")
        .body(Body::empty())
        .unwrap();
    let resp2 = app.oneshot(req2).await.unwrap();
    assert_eq!(resp2.status(), StatusCode::OK);

    let body2 = axum::body::to_bytes(resp2.into_body(), usize::MAX).await.unwrap();
    let json2: serde_json::Value = serde_json::from_slice(&body2).unwrap();

    let deducted: Decimal = json2["deducted_amount"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(
        deducted,
        Decimal::new(7500, 0),
        "deducted_amount must still be 7500 after merchant merge, got {}",
        deducted
    );

    let settlement: Decimal = json2["settlement_input"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let recognized: Decimal = json2["recognized_expense"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(
        settlement,
        recognized - deducted,
        "settlement_input must equal recognized_expense - deducted_amount after merge"
    );
}

// ── Test 7: delete_alias_removes_only_alias_row ───────────────────────────────

/// DELETE /api/aliases/:id must remove the alias row and not touch transactions.
#[sqlx::test(migrations = "./migrations")]
async fn delete_alias_removes_only_alias_row(pool: PgPool) {
    let pool = Arc::new(pool);
    let owner_id = Uuid::new_v4();
    do_import(&pool, owner_id).await;

    // Pick any alias.
    let alias: (Uuid, Uuid) = sqlx::query!(
        r#"SELECT id AS "id!: Uuid", target_id AS "target_id!: Uuid"
           FROM aliases WHERE owner_id = $1 AND scope = 'merchant' LIMIT 1"#,
        owner_id
    )
    .fetch_optional(&*pool)
    .await
    .unwrap()
    .map(|r| (r.id, r.target_id))
    .expect("should have at least one merchant alias after import");

    let (alias_id, target_id) = alias;

    let tx_count_before: i64 = sqlx::query_scalar!(
        r#"SELECT COUNT(*) FROM transactions WHERE owner_id = $1 AND merchant_id = $2"#,
        owner_id, target_id
    )
    .fetch_one(&*pool)
    .await
    .unwrap()
    .unwrap_or(0);

    let app = build_test_router(pool.clone(), owner_id);
    let req = Request::builder()
        .method("DELETE")
        .uri(format!("/api/aliases/{}", alias_id))
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // Alias row gone.
    let alias_exists: bool = sqlx::query_scalar!(
        r#"SELECT EXISTS(SELECT 1 FROM aliases WHERE id = $1) AS "e!: bool""#,
        alias_id
    )
    .fetch_one(&*pool)
    .await
    .unwrap();
    assert!(!alias_exists);

    // Transactions untouched.
    let tx_count_after: i64 = sqlx::query_scalar!(
        r#"SELECT COUNT(*) FROM transactions WHERE owner_id = $1 AND merchant_id = $2"#,
        owner_id, target_id
    )
    .fetch_one(&*pool)
    .await
    .unwrap()
    .unwrap_or(0);
    assert_eq!(tx_count_before, tx_count_after, "transactions must be untouched by alias delete");
}

// ── Test 8: review_queue_returns_pending_items ────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn review_queue_returns_pending_items(pool: PgPool) {
    let pool = Arc::new(pool);
    let owner_id = Uuid::new_v4();
    do_import(&pool, owner_id).await;

    let app = build_test_router(pool.clone(), owner_id);
    let req = Request::builder()
        .uri("/api/review-queue?scope=merchant")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let arr = json.as_array().expect("array response");
    // After import, merchants are pending — there must be pending items.
    assert!(!arr.is_empty(), "review queue must have pending merchants after import");
    for item in arr {
        assert_eq!(item["review_state"].as_str(), Some("pending"));
        assert_eq!(item["scope"].as_str(), Some("merchant"));
        assert!(item["id"].as_str().is_some());
        assert!(item["name"].as_str().is_some());
    }
}

// ── Test 9: confirm_merchant_flips_review_state ───────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn confirm_merchant_flips_review_state(pool: PgPool) {
    let pool = Arc::new(pool);
    let owner_id = Uuid::new_v4();
    do_import(&pool, owner_id).await;

    // Find a pending merchant.
    let merchant_id: Uuid = sqlx::query_scalar!(
        r#"SELECT id FROM merchants WHERE owner_id = $1 AND review_state = 'pending' LIMIT 1"#,
        owner_id
    )
    .fetch_one(&*pool)
    .await
    .unwrap();

    let app = build_test_router(pool.clone(), owner_id);
    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/entities/merchant/{}/confirm", merchant_id))
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["review_state"].as_str(), Some("confirmed"));

    // Verify in DB.
    let state: String = sqlx::query_scalar!(
        r#"SELECT review_state FROM merchants WHERE id = $1"#,
        merchant_id
    )
    .fetch_one(&*pool)
    .await
    .unwrap();
    assert_eq!(state, "confirmed");
}

// ── Test 10: confirm_payment_method_flips_review_state (M4-A) ────────────────

/// After importing the golden file, payment_methods are auto-created with
/// review_state='pending'. Confirming via POST /api/entities/payment_method/:id/confirm
/// must flip the row to 'confirmed', and /api/review-queue?scope=payment_method
/// must drop the row from pending.
#[sqlx::test(migrations = "./migrations")]
async fn confirm_payment_method_flips_review_state(pool: PgPool) {
    let pool = Arc::new(pool);
    let owner_id = Uuid::new_v4();
    do_import(&pool, owner_id).await;

    let pm_id: Uuid = sqlx::query_scalar!(
        r#"SELECT id FROM payment_methods WHERE owner_id = $1 AND review_state = 'pending' LIMIT 1"#,
        owner_id
    )
    .fetch_one(&*pool)
    .await
    .unwrap();

    // Confirm.
    let app = build_test_router(pool.clone(), owner_id);
    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/entities/payment_method/{}/confirm", pm_id))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["review_state"].as_str(), Some("confirmed"));

    // DB confirmation.
    let state: String = sqlx::query_scalar!(
        r#"SELECT review_state FROM payment_methods WHERE id = $1"#,
        pm_id
    )
    .fetch_one(&*pool)
    .await
    .unwrap();
    assert_eq!(state, "confirmed");

    // Review queue must no longer include this id.
    let req2 = Request::builder()
        .uri("/api/review-queue?scope=payment_method")
        .body(Body::empty())
        .unwrap();
    let resp2 = app.oneshot(req2).await.unwrap();
    assert_eq!(resp2.status(), StatusCode::OK);
    let body2 = axum::body::to_bytes(resp2.into_body(), usize::MAX).await.unwrap();
    let arr: Vec<serde_json::Value> = serde_json::from_slice(&body2).unwrap();
    assert!(
        arr.iter().all(|v| v["id"].as_str() != Some(pm_id.to_string().as_str())),
        "confirmed payment method must no longer appear in the pending review queue"
    );
}

// ── Test 11: review_queue_payment_method_returns_pending (M4-A) ──────────────

/// /api/review-queue?scope=payment_method must return the freshly imported
/// (pending) payment methods after the golden import.
#[sqlx::test(migrations = "./migrations")]
async fn review_queue_payment_method_returns_pending(pool: PgPool) {
    let pool = Arc::new(pool);
    let owner_id = Uuid::new_v4();
    do_import(&pool, owner_id).await;

    let app = build_test_router(pool.clone(), owner_id);
    let req = Request::builder()
        .uri("/api/review-queue?scope=payment_method")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let arr: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
    assert!(
        !arr.is_empty(),
        "review queue for payment_method must be non-empty after import"
    );
    for item in &arr {
        assert_eq!(item["scope"].as_str(), Some("payment_method"));
        assert_eq!(item["review_state"].as_str(), Some("pending"));
    }
}
