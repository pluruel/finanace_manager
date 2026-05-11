/// Integration tests for GET /api/summary/income/:year/:month
///
/// Verifies per-actor income totals, zero-fill for actors with no income,
/// and the "month" field format.

mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
    middleware, routing, Router,
};
use finance_manager::auth::AuthUser;
use sea_orm::{DatabaseBackend, DatabaseConnection, FromQueryResult, Statement};
use std::sync::Arc;
use tower::ServiceExt;
use uuid::Uuid;

// ── Router ────────────────────────────────────────────────────────────────────

fn build_test_router(db: Arc<DatabaseConnection>, owner_id: Uuid) -> Router {
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

// ── Helper ────────────────────────────────────────────────────────────────────

#[derive(FromQueryResult)]
struct IdRow { id: Uuid }

async fn insert_returning_id(
    db: &sea_orm::DatabaseConnection,
    sql: &str,
    values: Vec<sea_orm::Value>,
) -> Uuid {
    IdRow::find_by_statement(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        sql,
        values,
    ))
    .one(db)
    .await
    .unwrap()
    .unwrap()
    .id
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// Happy path: one income transaction for 엉아; 공동 and 아기 should be zero-filled.
#[tokio::test]
async fn income_by_actor_one_actor_has_income() {
    let t = common::TestDb::new().await;
    let owner_id = Uuid::new_v4();
    let db = Arc::clone(&t.db);

    // Create three ledger actors
    let _actor_gongjong = insert_returning_id(
        &*t.db,
        "INSERT INTO ledger_actors (owner_id, name) VALUES ($1, '공동') RETURNING id",
        vec![owner_id.into()],
    ).await;

    let actor_eonga = insert_returning_id(
        &*t.db,
        "INSERT INTO ledger_actors (owner_id, name) VALUES ($1, '엉아') RETURNING id",
        vec![owner_id.into()],
    ).await;

    let _actor_baby = insert_returning_id(
        &*t.db,
        "INSERT INTO ledger_actors (owner_id, name) VALUES ($1, '아기') RETURNING id",
        vec![owner_id.into()],
    ).await;

    // Create an income category
    let category_id = insert_returning_id(
        &*t.db,
        "INSERT INTO categories (owner_id, name, kind) VALUES ($1, '급여', 'income') RETURNING id",
        vec![owner_id.into()],
    ).await;

    // Insert minimal import_batch + transactions_raw to satisfy the FK chain
    let batch_id = insert_returning_id(
        &*t.db,
        r#"INSERT INTO import_batches (owner_id, file_name, file_hash, year, month, row_count)
           VALUES ($1, 'test.xlsx', '\x00'::bytea, 2026, 2, 1)
           RETURNING id"#,
        vec![owner_id.into()],
    ).await;

    let group_id = Uuid::new_v4();

    let raw_id = insert_returning_id(
        &*t.db,
        r#"INSERT INTO transactions_raw
           (owner_id, import_batch_id, row_index, group_id, is_group_header)
           VALUES ($1, $2, 0, $3, true)
           RETURNING id"#,
        vec![owner_id.into(), batch_id.into(), group_id.into()],
    ).await;

    // Insert one income transaction for 엉아 (amount positive = cash inflow)
    use sea_orm::ConnectionTrait;
    t.db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"INSERT INTO transactions
           (owner_id, raw_id, group_id, occurred_on, actor_id, category_id, amount)
           VALUES ($1, $2, $3, '2026-02-25', $4, $5, 3500000)"#,
        vec![owner_id.into(), raw_id.into(), group_id.into(), actor_eonga.into(), category_id.into()],
    )).await.unwrap();

    // Build router and call the endpoint
    let app = build_test_router(db, owner_id);

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

/// `categories` 필드는 income kind 카테고리만 포함하고 액터 셀 합계가 양수.
#[tokio::test]
async fn income_response_includes_categories_breakdown() {
    let t = common::TestDb::new().await;
    let owner_id = Uuid::new_v4();
    let db = Arc::clone(&t.db);

    use sea_orm::ConnectionTrait;

    let actor_eonga = insert_returning_id(
        &*t.db,
        "INSERT INTO ledger_actors (owner_id, name) VALUES ($1, '엉아') RETURNING id",
        vec![owner_id.into()],
    ).await;

    let salary_cat = insert_returning_id(
        &*t.db,
        "INSERT INTO categories (owner_id, name, kind) VALUES ($1, '급여', 'income') RETURNING id",
        vec![owner_id.into()],
    ).await;

    let batch_id = insert_returning_id(
        &*t.db,
        r#"INSERT INTO import_batches (owner_id, file_name, file_hash, year, month, row_count)
           VALUES ($1, 'cat.xlsx', '\x02'::bytea, 2026, 2, 1)
           RETURNING id"#,
        vec![owner_id.into()],
    ).await;

    let group_id = Uuid::new_v4();
    let raw_id = insert_returning_id(
        &*t.db,
        r#"INSERT INTO transactions_raw
           (owner_id, import_batch_id, row_index, group_id, is_group_header)
           VALUES ($1, $2, 0, $3, true) RETURNING id"#,
        vec![owner_id.into(), batch_id.into(), group_id.into()],
    ).await;

    t.db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"INSERT INTO transactions
           (owner_id, raw_id, group_id, occurred_on, actor_id, category_id, amount)
           VALUES ($1, $2, $3, '2026-02-25', $4, $5, 4500000)"#,
        vec![owner_id.into(), raw_id.into(), group_id.into(), actor_eonga.into(), salary_cat.into()],
    )).await.unwrap();

    let app = build_test_router(db, owner_id);
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

    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

    let categories = body["categories"].as_array().expect("categories must be an array");
    assert_eq!(categories.len(), 1, "only one income category expected");
    assert_eq!(categories[0]["category_name"], "급여");
    assert_eq!(categories[0]["kind"], "income");

    let by_actor = categories[0]["by_actor"].as_array().unwrap();
    assert_eq!(by_actor.len(), 1);
    assert_eq!(by_actor[0]["actor_name"], "엉아");
    let amt: rust_decimal::Decimal = by_actor[0]["amount"].as_str().unwrap().parse().unwrap();
    assert_eq!(amt, "4500000".parse::<rust_decimal::Decimal>().unwrap(),
               "income amount stays positive (no sign flip)");
}

/// expense kind 카테고리는 categories 에 절대 등장하지 않는다.
#[tokio::test]
async fn income_categories_exclude_expense_kind() {
    let t = common::TestDb::new().await;
    let owner_id = Uuid::new_v4();
    let db = Arc::clone(&t.db);

    use sea_orm::ConnectionTrait;

    let actor = insert_returning_id(
        &*t.db,
        "INSERT INTO ledger_actors (owner_id, name) VALUES ($1, '엉아') RETURNING id",
        vec![owner_id.into()],
    ).await;

    let income_cat = insert_returning_id(
        &*t.db,
        "INSERT INTO categories (owner_id, name, kind) VALUES ($1, '급여', 'income') RETURNING id",
        vec![owner_id.into()],
    ).await;

    let expense_cat = insert_returning_id(
        &*t.db,
        "INSERT INTO categories (owner_id, name, kind) VALUES ($1, '식비', 'expense') RETURNING id",
        vec![owner_id.into()],
    ).await;

    let batch_id = insert_returning_id(
        &*t.db,
        r#"INSERT INTO import_batches (owner_id, file_name, file_hash, year, month, row_count)
           VALUES ($1, 'mix.xlsx', '\x03'::bytea, 2026, 2, 2)
           RETURNING id"#,
        vec![owner_id.into()],
    ).await;

    for (i, (cat, amount)) in [(income_cat, 1000000_i64), (expense_cat, -50000_i64)].iter().enumerate() {
        let group_id = Uuid::new_v4();
        let raw_id = insert_returning_id(
            &*t.db,
            r#"INSERT INTO transactions_raw
               (owner_id, import_batch_id, row_index, group_id, is_group_header)
               VALUES ($1, $2, $3, $4, true) RETURNING id"#,
            vec![owner_id.into(), batch_id.into(), (i as i64).into(), group_id.into()],
        ).await;

        t.db.execute(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"INSERT INTO transactions
               (owner_id, raw_id, group_id, occurred_on, actor_id, category_id, amount)
               VALUES ($1, $2, $3, '2026-02-15', $4, $5, $6)"#,
            vec![
                owner_id.into(), raw_id.into(), group_id.into(), actor.into(), (*cat).into(),
                rust_decimal::Decimal::from(*amount).into(),
            ],
        )).await.unwrap();
    }

    let app = build_test_router(db, owner_id);
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

    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

    let categories = body["categories"].as_array().unwrap();
    assert_eq!(categories.len(), 1, "only the income category should appear");
    assert_eq!(categories[0]["category_name"], "급여");
    assert_eq!(categories[0]["kind"], "income");
    for c in categories {
        assert_ne!(c["kind"], "expense", "expense kind must not leak into income response");
    }
}

/// Expense-only transactions should not count towards income totals.
#[tokio::test]
async fn expense_transactions_excluded_from_income() {
    let t = common::TestDb::new().await;
    let owner_id = Uuid::new_v4();
    let db = Arc::clone(&t.db);

    use sea_orm::ConnectionTrait;

    let _actor = insert_returning_id(
        &*t.db,
        "INSERT INTO ledger_actors (owner_id, name) VALUES ($1, '엉아') RETURNING id",
        vec![owner_id.into()],
    ).await;

    let expense_cat = insert_returning_id(
        &*t.db,
        "INSERT INTO categories (owner_id, name, kind) VALUES ($1, '식비', 'expense') RETURNING id",
        vec![owner_id.into()],
    ).await;

    let batch_id = insert_returning_id(
        &*t.db,
        r#"INSERT INTO import_batches (owner_id, file_name, file_hash, year, month, row_count)
           VALUES ($1, 'test2.xlsx', '\x01'::bytea, 2026, 2, 1)
           RETURNING id"#,
        vec![owner_id.into()],
    ).await;

    let group_id = Uuid::new_v4();

    let raw_id = insert_returning_id(
        &*t.db,
        r#"INSERT INTO transactions_raw
           (owner_id, import_batch_id, row_index, group_id, is_group_header)
           VALUES ($1, $2, 0, $3, true)
           RETURNING id"#,
        vec![owner_id.into(), batch_id.into(), group_id.into()],
    ).await;

    // Insert an expense transaction (negative amount = cash outflow)
    t.db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"INSERT INTO transactions
           (owner_id, raw_id, group_id, occurred_on, category_id, amount)
           VALUES ($1, $2, $3, '2026-02-10', $4, -15000)"#,
        vec![owner_id.into(), raw_id.into(), group_id.into(), expense_cat.into()],
    )).await.unwrap();

    let app = build_test_router(db, owner_id);

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
