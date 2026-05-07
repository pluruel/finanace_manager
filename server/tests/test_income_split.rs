/// Integration tests for GET /api/summary/income/:year/:month
///
/// Verifies per-actor income totals, zero-fill for actors with no income,
/// and the "month" field format.

use axum::{
    body::Body,
    http::{Request, StatusCode},
    middleware, routing, Router,
};
use finance_manager::auth::AuthUser;
use sqlx::PgPool;
use std::sync::Arc;
use tower::ServiceExt;
use uuid::Uuid;

// ── Router ────────────────────────────────────────────────────────────────────

fn build_test_router(pool: Arc<PgPool>, owner_id: Uuid) -> Router {
    let user = AuthUser {
        sub: owner_id,
        email: "test@example.com".to_string(),
        groups: vec![],
    };

    Router::new()
        .route(
            "/api/summary/income/:year/:month",
            routing::get(finance_manager::api::income::handle_get_income),
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

// ── Tests ─────────────────────────────────────────────────────────────────────

/// Happy path: one income transaction for 엉아; 공동 and 아기 should be zero-filled.
#[sqlx::test(migrations = "./migrations")]
async fn income_by_actor_one_actor_has_income(pool: PgPool) {
    let owner_id = Uuid::new_v4();
    let pool = Arc::new(pool);

    // Create three ledger actors
    let actor_gongjong = sqlx::query_scalar!(
        "INSERT INTO ledger_actors (owner_id, name) VALUES ($1, '공동') RETURNING id",
        owner_id
    )
    .fetch_one(&*pool)
    .await
    .unwrap();

    let actor_eonga = sqlx::query_scalar!(
        "INSERT INTO ledger_actors (owner_id, name) VALUES ($1, '엉아') RETURNING id",
        owner_id
    )
    .fetch_one(&*pool)
    .await
    .unwrap();

    let _actor_baby: Uuid = sqlx::query_scalar!(
        "INSERT INTO ledger_actors (owner_id, name) VALUES ($1, '아기') RETURNING id",
        owner_id
    )
    .fetch_one(&*pool)
    .await
    .unwrap();

    // Create an income category
    let category_id: Uuid = sqlx::query_scalar!(
        "INSERT INTO categories (owner_id, name, kind) VALUES ($1, '급여', 'income') RETURNING id",
        owner_id
    )
    .fetch_one(&*pool)
    .await
    .unwrap();

    // Insert minimal import_batch + transactions_raw to satisfy the FK chain
    let batch_id: Uuid = sqlx::query_scalar!(
        r#"INSERT INTO import_batches (owner_id, file_name, file_hash, year, month, row_count)
           VALUES ($1, 'test.xlsx', '\x00'::bytea, 2026, 2, 1)
           RETURNING id"#,
        owner_id
    )
    .fetch_one(&*pool)
    .await
    .unwrap();

    let group_id = Uuid::new_v4();

    let raw_id: Uuid = sqlx::query_scalar!(
        r#"INSERT INTO transactions_raw
           (owner_id, import_batch_id, row_index, group_id, is_group_header)
           VALUES ($1, $2, 0, $3, true)
           RETURNING id"#,
        owner_id,
        batch_id,
        group_id,
    )
    .fetch_one(&*pool)
    .await
    .unwrap();

    // Insert one income transaction for 엉아 (amount positive = cash inflow)
    sqlx::query!(
        r#"INSERT INTO transactions
           (owner_id, raw_id, group_id, occurred_on, actor_id, category_id, amount)
           VALUES ($1, $2, $3, '2026-02-25', $4, $5, 3500000)"#,
        owner_id,
        raw_id,
        group_id,
        actor_eonga,
        category_id,
    )
    .execute(&*pool)
    .await
    .unwrap();

    // Build router and call the endpoint
    let app = build_test_router(pool, owner_id);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/summary/income/2026/2")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

    // month field
    assert_eq!(body["month"], "2026-02");

    // total — parse as Decimal to avoid depending on exact scale formatting
    let total: rust_decimal::Decimal = body["total"].as_str().unwrap().parse().unwrap();
    assert_eq!(total, "3500000".parse::<rust_decimal::Decimal>().unwrap());

    // by_actor: all three actors present (sorted by name)
    let by_actor = body["by_actor"].as_array().unwrap();
    assert_eq!(by_actor.len(), 3, "all three actors must be present");

    // actors sorted by name: 공동, 아기, 엉아 (Korean sort)
    // find by actor_name for stability
    let find_actor = |name: &str| {
        by_actor
            .iter()
            .find(|a| a["actor_name"].as_str().unwrap() == name)
            .unwrap_or_else(|| panic!("actor '{}' not found in by_actor", name))
            .clone()
    };

    // Parse the string representation to compare as Decimal (avoids depending on exact scale format)
    let gongjong = find_actor("공동");
    let gongjong_total: rust_decimal::Decimal =
        gongjong["total"].as_str().unwrap().parse().unwrap();
    assert_eq!(gongjong_total, rust_decimal::Decimal::ZERO, "공동 should have zero income");

    let eonga = find_actor("엉아");
    let eonga_total: rust_decimal::Decimal = eonga["total"].as_str().unwrap().parse().unwrap();
    assert_eq!(
        eonga_total,
        "3500000".parse::<rust_decimal::Decimal>().unwrap(),
        "엉아 should have 3500000 income"
    );

    let baby = find_actor("아기");
    let baby_total: rust_decimal::Decimal = baby["total"].as_str().unwrap().parse().unwrap();
    assert_eq!(baby_total, rust_decimal::Decimal::ZERO, "아기 should have zero income");
}

/// Expense-only transactions should not count towards income totals.
#[sqlx::test(migrations = "./migrations")]
async fn expense_transactions_excluded_from_income(pool: PgPool) {
    let owner_id = Uuid::new_v4();
    let pool = Arc::new(pool);

    let _actor: Uuid = sqlx::query_scalar!(
        "INSERT INTO ledger_actors (owner_id, name) VALUES ($1, '엉아') RETURNING id",
        owner_id
    )
    .fetch_one(&*pool)
    .await
    .unwrap();

    let expense_cat: Uuid = sqlx::query_scalar!(
        "INSERT INTO categories (owner_id, name, kind) VALUES ($1, '식비', 'expense') RETURNING id",
        owner_id
    )
    .fetch_one(&*pool)
    .await
    .unwrap();

    let batch_id: Uuid = sqlx::query_scalar!(
        r#"INSERT INTO import_batches (owner_id, file_name, file_hash, year, month, row_count)
           VALUES ($1, 'test2.xlsx', '\x01'::bytea, 2026, 2, 1)
           RETURNING id"#,
        owner_id
    )
    .fetch_one(&*pool)
    .await
    .unwrap();

    let group_id = Uuid::new_v4();

    let raw_id: Uuid = sqlx::query_scalar!(
        r#"INSERT INTO transactions_raw
           (owner_id, import_batch_id, row_index, group_id, is_group_header)
           VALUES ($1, $2, 0, $3, true)
           RETURNING id"#,
        owner_id,
        batch_id,
        group_id,
    )
    .fetch_one(&*pool)
    .await
    .unwrap();

    // Insert an expense transaction (negative amount = cash outflow)
    sqlx::query!(
        r#"INSERT INTO transactions
           (owner_id, raw_id, group_id, occurred_on, category_id, amount)
           VALUES ($1, $2, $3, '2026-02-10', $4, -15000)"#,
        owner_id,
        raw_id,
        group_id,
        expense_cat,
    )
    .execute(&*pool)
    .await
    .unwrap();

    let app = build_test_router(pool, owner_id);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/summary/income/2026/2")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

    assert_eq!(body["month"], "2026-02");
    let total: rust_decimal::Decimal = body["total"].as_str().unwrap().parse().unwrap();
    assert_eq!(total, rust_decimal::Decimal::ZERO);

    let by_actor = body["by_actor"].as_array().unwrap();
    for actor in by_actor {
        let actor_total: rust_decimal::Decimal =
            actor["total"].as_str().unwrap().parse().unwrap();
        assert_eq!(
            actor_total,
            rust_decimal::Decimal::ZERO,
            "no income expected for any actor"
        );
    }
}
