/// 테스트 3: 임포트 통합 (DB 사용)
///
/// #[sqlx::test] 매크로: 각 테스트마다 임시 DB 생성 + migrations 자동 실행 + 테스트 후 정리
///
/// 검증:
/// - 골든 xlsx 임포트 → transactions 카운트 177
/// - product_id IS NULL 행 수 = 63 (메모 없는 행)
/// - 그룹 합계 무결성 SQL 0건
/// - v_monthly_settlement 2026-02-01 deducted_amount=7500
/// - 동일 파일 두 번 임포트 → 두 번째는 409 + DB 상태 불변

use finance_manager::import::xlsx::{extract_sheet_name, extract_year_month, parse_xlsx};
use finance_manager::import::pipeline::run_pipeline;
use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

fn load_golden_bytes() -> Vec<u8> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/2026년_02월.xlsx"
    );
    std::fs::read(path).expect("Failed to read golden xlsx fixture")
}

/// 임포트 파이프라인을 직접 실행해 결과를 반환한다.
/// 실제 HTTP 레이어를 건너뛰고 pipeline + DB만 검증.
async fn run_import(
    pool: &PgPool,
    owner_id: Uuid,
    bytes: &[u8],
    filename: &str,
) -> anyhow::Result<(Uuid, i64, Vec<finance_manager::domain::IntegrityWarning>)> {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let hash_vec = hasher.finalize().to_vec();

    let (year, month) = extract_year_month(filename).expect("filename parse failed");
    let sheet_name = extract_sheet_name(filename).expect("sheet_name parse failed");
    let raw_rows = parse_xlsx(bytes, &sheet_name)?;
    let row_count = raw_rows.len() as i32;

    let mut tx = pool.begin().await?;

    let batch_id: Option<Uuid> = sqlx::query_scalar!(
        r#"INSERT INTO import_batches (owner_id, file_name, file_hash, year, month, row_count)
           VALUES ($1, $2, $3, $4, $5, $6)
           ON CONFLICT (owner_id, file_hash) DO NOTHING
           RETURNING id"#,
        owner_id,
        filename,
        hash_vec,
        year,
        month,
        row_count,
    )
    .fetch_optional(&mut *tx)
    .await?;

    let batch_id = match batch_id {
        Some(id) => id,
        None => anyhow::bail!("CONFLICT: already imported"),
    };

    let (transactions_inserted, integrity_warnings, _) =
        run_pipeline(&mut *tx, owner_id, batch_id, raw_rows).await?;

    tx.commit().await?;

    Ok((batch_id, transactions_inserted, integrity_warnings))
}

#[sqlx::test(migrations = "./migrations")]
async fn import_golden_transactions_count(pool: PgPool) {
    let owner_id = Uuid::new_v4();
    let bytes = load_golden_bytes();
    let filename = "2026년 02월.xlsx";

    let (_, tx_count, warnings) = run_import(&pool, owner_id, &bytes, filename)
        .await
        .expect("import failed");

    // 177개 유효 행 → 177개 transactions
    assert_eq!(
        tx_count, 177,
        "Expected 177 transactions, got {}",
        tx_count
    );

    // 그룹 합계 무결성 위반 0건
    assert_eq!(
        warnings.len(),
        0,
        "Expected 0 integrity warnings, got {}: {:?}",
        warnings.len(),
        warnings
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn import_golden_product_null_count(pool: PgPool) {
    let owner_id = Uuid::new_v4();
    let bytes = load_golden_bytes();
    let filename = "2026년 02월.xlsx";

    run_import(&pool, owner_id, &bytes, filename)
        .await
        .expect("import failed");

    // product_id IS NULL 행 수:
    // - 메모 없는 행: 63
    // - 메모 있지만 merchant 없는 행: 1 (Row 24: merchant=None, memo 있음)
    // 합계 = 64
    let null_count: i64 = sqlx::query_scalar!(
        r#"SELECT COUNT(*)::bigint FROM transactions WHERE owner_id = $1 AND product_id IS NULL"#,
        owner_id
    )
    .fetch_one(&pool)
    .await
    .expect("query failed")
    .unwrap_or(0);

    assert_eq!(
        null_count, 64,
        "Expected 64 rows with product_id IS NULL (63 no-memo + 1 no-merchant), got {}",
        null_count
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn import_golden_integrity_sql_zero_violations(pool: PgPool) {
    let owner_id = Uuid::new_v4();
    let bytes = load_golden_bytes();
    let filename = "2026년 02월.xlsx";

    let (batch_id, _, _) = run_import(&pool, owner_id, &bytes, filename)
        .await
        .expect("import failed");

    // 그룹 합계 무결성 SQL: 불일치 그룹 0건
    let violations: i64 = sqlx::query_scalar!(
        r#"
        SELECT COUNT(*)::bigint
        FROM (
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
        ) violations
        "#,
        owner_id,
        batch_id,
    )
    .fetch_one(&pool)
    .await
    .expect("integrity SQL failed")
    .unwrap_or(0);

    assert_eq!(
        violations, 0,
        "그룹 합계 무결성 SQL에서 {} 건의 위반이 검출됨",
        violations
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn import_golden_settlement_deducted_amount(pool: PgPool) {
    let owner_id = Uuid::new_v4();
    let bytes = load_golden_bytes();
    let filename = "2026년 02월.xlsx";

    run_import(&pool, owner_id, &bytes, filename)
        .await
        .expect("import failed");

    // v_monthly_settlement: 2026-02-01, deducted_amount = 7500
    let row = sqlx::query!(
        r#"
        SELECT
            recognized_expense AS "recognized_expense!: Decimal",
            deducted_amount     AS "deducted_amount!: Decimal",
            settlement_input    AS "settlement_input!: Decimal"
        FROM v_monthly_settlement
        WHERE owner_id = $1 AND month = '2026-02-01'
        "#,
        owner_id
    )
    .fetch_one(&pool)
    .await
    .expect("v_monthly_settlement query failed");

    let expected_deducted = Decimal::new(7500, 0);
    assert_eq!(
        row.deducted_amount, expected_deducted,
        "deducted_amount 불일치: expected={}, got={}",
        expected_deducted, row.deducted_amount
    );

    // settlement_input = recognized_expense - deducted_amount 검증
    let expected_settlement =
        row.recognized_expense - row.deducted_amount;
    assert_eq!(
        row.settlement_input, expected_settlement,
        "settlement_input = recognized_expense - deducted_amount 불일치: {} - {} = {} (got {})",
        row.recognized_expense,
        row.deducted_amount,
        expected_settlement,
        row.settlement_input
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn import_duplicate_returns_conflict(pool: PgPool) {
    let owner_id = Uuid::new_v4();
    let bytes = load_golden_bytes();
    let filename = "2026년 02월.xlsx";

    // 1차 임포트 성공
    let (_, tx_count_1, _) = run_import(&pool, owner_id, &bytes, filename)
        .await
        .expect("first import failed");

    assert_eq!(tx_count_1, 177);

    // 2차 임포트 → CONFLICT 에러
    let result = run_import(&pool, owner_id, &bytes, filename).await;
    assert!(
        result.is_err(),
        "두 번째 임포트는 에러를 반환해야 한다"
    );
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("CONFLICT"),
        "에러 메시지에 CONFLICT 포함 기대: {}",
        err_msg
    );

    // DB 상태 불변: transactions 수 동일
    let tx_count_after: i64 = sqlx::query_scalar!(
        r#"SELECT COUNT(*)::bigint FROM transactions WHERE owner_id = $1"#,
        owner_id
    )
    .fetch_one(&pool)
    .await
    .expect("count query failed")
    .unwrap_or(0);

    assert_eq!(
        tx_count_after, 177,
        "두 번째 임포트 시도 후 transactions 수가 변해서는 안 된다"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn import_single_batch_verified(pool: PgPool) {
    // 단일 트랜잭션 임포트: import_batches가 정확히 1건만 생성됨
    let owner_id = Uuid::new_v4();
    let bytes = load_golden_bytes();
    let filename = "2026년 02월.xlsx";

    run_import(&pool, owner_id, &bytes, filename)
        .await
        .expect("import failed");

    let batch_count: i64 = sqlx::query_scalar!(
        r#"SELECT COUNT(*)::bigint FROM import_batches WHERE owner_id = $1"#,
        owner_id
    )
    .fetch_one(&pool)
    .await
    .expect("query failed")
    .unwrap_or(0);

    assert_eq!(
        batch_count, 1,
        "import_batches는 정확히 1건이어야 한다, got {}",
        batch_count
    );
}
