/// M4-B: xlsx export tests
///
/// Verifies GET /api/export/:year/:month returns a valid xlsx with the
/// expected sheets and that the Settlement sheet matches the underlying
/// v_monthly_settlement values.

mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
    middleware, routing, Router,
};
use calamine::{open_workbook_from_rs, DataType, Reader, Xlsx};
use finance_manager::auth::AuthUser;
use finance_manager::import::pipeline::run_pipeline;
use finance_manager::import::xlsx::{extract_sheet_name, extract_year_month, parse_xlsx};
use sea_orm::DatabaseConnection;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::io::Cursor;
use std::sync::Arc;
use tower::ServiceExt;
use uuid::Uuid;

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

fn build_router(db: Arc<DatabaseConnection>, owner_id: Uuid) -> Router {
    let user = AuthUser {
        sub: owner_id,
        email: "test@example.com".to_string(),
        groups: vec![],
    };

    Router::new()
        .route(
            "/api/export/:year/:month",
            routing::get(finance_manager::api::export::handle_get_export),
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

#[tokio::test]
async fn export_returns_valid_xlsx_with_three_sheets() {
    let t = common::TestDb::new().await;
    let pool = &t.pool;
    let owner_id = Uuid::new_v4();
    do_import(pool, owner_id).await;

    let db = Arc::clone(&t.db);
    let app = build_router(Arc::clone(&db), owner_id);
    let req = Request::builder()
        .uri("/api/export/2026/2")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let ct = resp
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        ct.contains("spreadsheetml.sheet"),
        "Content-Type must be xlsx, got: {ct}"
    );

    let cd = resp
        .headers()
        .get(axum::http::header::CONTENT_DISPOSITION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        cd.contains("attachment") && cd.contains("finance-2026-02.xlsx"),
        "Content-Disposition wrong: {cd}"
    );

    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let cursor = Cursor::new(body.to_vec());
    let mut wb: Xlsx<_> = open_workbook_from_rs(cursor).expect("xlsx must be parseable");
    let names = wb.sheet_names().to_vec();
    assert_eq!(names, vec!["Transactions", "Settlement", "Summary"]);

    // Settlement sheet must match what /api/settlement returns. Pull live values
    // from v_monthly_settlement and compare against the export cells.
    let live = sqlx::query!(
        r#"SELECT recognized_expense AS "r!: rust_decimal::Decimal",
                  deducted_amount    AS "d!: rust_decimal::Decimal",
                  settlement_input   AS "s!: rust_decimal::Decimal"
           FROM v_monthly_settlement
           WHERE owner_id = $1 AND month = '2026-02-01'"#,
        owner_id
    )
    .fetch_one(pool)
    .await
    .unwrap();
    let live_recognized: f64 = live.r.to_string().parse().unwrap();
    let live_deducted: f64 = live.d.to_string().parse().unwrap();
    let live_settlement: f64 = live.s.to_string().parse().unwrap();

    let s = wb.worksheet_range("Settlement").unwrap();
    let year: f64 = s.get_value((1, 0)).unwrap().as_f64().unwrap();
    let month: f64 = s.get_value((1, 1)).unwrap().as_f64().unwrap();
    let recognized: f64 = s.get_value((1, 2)).unwrap().as_f64().unwrap();
    let deducted: f64 = s.get_value((1, 3)).unwrap().as_f64().unwrap();
    let settlement: f64 = s.get_value((1, 4)).unwrap().as_f64().unwrap();
    assert_eq!(year as i32, 2026);
    assert_eq!(month as i32, 2);
    assert!((recognized - live_recognized).abs() < 0.5, "recognized mismatch");
    assert!(
        (deducted - 7_500.0).abs() < 0.5,
        "deducted_amount must match golden 7500 (got {deducted})"
    );
    assert!((deducted - live_deducted).abs() < 0.5, "deducted vs live mismatch");
    assert!((settlement - live_settlement).abs() < 0.5, "settlement vs live mismatch");

    // Transactions sheet must have at least one data row past the header.
    let t = wb.worksheet_range("Transactions").unwrap();
    let (rows, _cols) = t.get_size();
    assert!(rows > 100, "Transactions sheet should have many rows, got {rows}");
}

#[tokio::test]
async fn export_invalid_month_returns_400() {
    let t = common::TestDb::new().await;
    let owner_id = Uuid::new_v4();
    let db = Arc::clone(&t.db);
    let app = build_router(db, owner_id);

    let req = Request::builder()
        .uri("/api/export/2026/13")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn export_empty_month_returns_xlsx_with_zero_settlement() {
    let t = common::TestDb::new().await;
    let owner_id = Uuid::new_v4();
    // No import — request a month with no data.

    let db = Arc::clone(&t.db);
    let app = build_router(db, owner_id);
    let req = Request::builder()
        .uri("/api/export/2026/2")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let cursor = Cursor::new(body.to_vec());
    let mut wb: Xlsx<_> = open_workbook_from_rs(cursor).expect("xlsx must be parseable");
    let s = wb.worksheet_range("Settlement").unwrap();
    let recognized: f64 = s.get_value((1, 2)).unwrap().as_f64().unwrap();
    let deducted: f64 = s.get_value((1, 3)).unwrap().as_f64().unwrap();
    assert_eq!(recognized, 0.0);
    assert_eq!(deducted, 0.0);
}
