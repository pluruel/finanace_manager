/// M1 검증용 독립 바이너리
/// 실제 JWT 없이 pipeline을 직접 테스트
/// usage: DATABASE_URL=... cargo run --bin test_import
use uuid::Uuid;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("finance_manager=debug")
        .init();

    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://app:app@localhost:5432/finance".to_string());

    let pool = sqlx::PgPool::connect(&database_url).await?;

    // 테스트용 owner_id (실제 auth-svc sub UUID 형식)
    let owner_id = Uuid::parse_str("00000000-0000-0000-0000-000000000001")
        .expect("valid UUID");

    // xlsx 파일 읽기
    let xlsx_path = std::env::args().nth(1)
        .unwrap_or_else(|| "/Users/juno/dev/finance_mananger/2026년 02월.xlsx".to_string());

    println!("Reading xlsx: {}", xlsx_path);
    let bytes = std::fs::read(&xlsx_path)?;

    // SHA-256
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let hash_vec = hasher.finalize().to_vec();

    // 파일명 추출
    let file_name = std::path::Path::new(&xlsx_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("2026년 02월.xlsx")
        .to_string();

    use finance_manager::import::xlsx::{extract_sheet_name, extract_year_month, parse_xlsx};
    use finance_manager::import::pipeline::run_pipeline;

    let (year, month) = extract_year_month(&file_name)
        .expect("Cannot extract year/month from filename");
    let sheet_name = extract_sheet_name(&file_name)
        .expect("Cannot extract sheet name");

    println!("Year: {}, Month: {}, Sheet: {}", year, month, sheet_name);

    // xlsx 파싱
    let raw_rows = parse_xlsx(&bytes, &sheet_name)?;
    let row_count = raw_rows.len() as i32;
    println!("Parsed {} rows from xlsx", raw_rows.len());

    // 단일 트랜잭션으로 전체 파이프라인 실행
    let mut tx = pool.begin().await?;

    // import_batches 삽입
    let batch_id: Option<Uuid> = sqlx::query_scalar!(
        r#"INSERT INTO import_batches (owner_id, file_name, file_hash, year, month, row_count)
           VALUES ($1, $2, $3, $4, $5, $6)
           ON CONFLICT (owner_id, file_hash) DO NOTHING
           RETURNING id"#,
        owner_id,
        file_name,
        hash_vec,
        year,
        month,
        row_count,
    )
    .fetch_optional(&mut *tx)
    .await?;

    let batch_id = match batch_id {
        Some(id) => {
            println!("New import batch: {}", id);
            id
        }
        None => {
            println!("Duplicate file! Deleting old data and re-importing...");
            // 트랜잭션 롤백 후 기존 데이터 삭제 후 재임포트
            drop(tx);

            let existing_id: Uuid = sqlx::query_scalar!(
                r#"SELECT id FROM import_batches WHERE owner_id = $1 AND file_hash = $2"#,
                owner_id,
                hash_vec,
            )
            .fetch_one(&pool)
            .await?;

            sqlx::query!("DELETE FROM import_batches WHERE id = $1", existing_id)
                .execute(&pool)
                .await?;

            let mut tx2 = pool.begin().await?;
            let new_id: Uuid = sqlx::query_scalar!(
                r#"INSERT INTO import_batches (owner_id, file_name, file_hash, year, month, row_count)
                   VALUES ($1, $2, $3, $4, $5, $6)
                   RETURNING id"#,
                owner_id,
                file_name,
                hash_vec,
                year,
                month,
                row_count,
            )
            .fetch_one(&mut *tx2)
            .await?;

            let (transactions_inserted, integrity_warnings, unresolved) =
                run_pipeline(&mut *tx2, owner_id, new_id, raw_rows).await?;

            tx2.commit().await?;

            println!("Re-import batch: {}", new_id);
            print_results(transactions_inserted, &integrity_warnings, &unresolved);
            return verify_db(&pool, owner_id).await;
        }
    };

    // 파이프라인 실행
    let (transactions_inserted, integrity_warnings, unresolved) =
        run_pipeline(&mut *tx, owner_id, batch_id, raw_rows).await?;

    tx.commit().await?;

    print_results(transactions_inserted, &integrity_warnings, &unresolved);
    verify_db(&pool, owner_id).await
}

fn print_results(
    transactions_inserted: i64,
    integrity_warnings: &[finance_manager::domain::IntegrityWarning],
    unresolved: &[finance_manager::domain::UnresolvedAlias],
) {
    println!("\n=== 임포트 결과 ===");
    println!("transactions 삽입: {}", transactions_inserted);
    println!("그룹 무결성 경고: {}", integrity_warnings.len());

    if integrity_warnings.is_empty() {
        println!("  0행 (정상)");
    } else {
        for w in integrity_warnings {
            println!("  WARN group_id={}, header={}, lines_sum={}", w.group_id, w.header_total, w.lines_sum);
        }
    }

    println!("unresolved aliases: {}", unresolved.len());
    for u in unresolved {
        println!("  {} | {} | {}", u.scope, u.raw_text, u.norm_key);
    }
}

async fn verify_db(pool: &sqlx::PgPool, owner_id: uuid::Uuid) -> anyhow::Result<()> {
    println!("\n=== DB 검증 ===");

    let txn_count: i64 = sqlx::query_scalar!(
        "SELECT COUNT(*) FROM transactions WHERE owner_id = $1",
        owner_id
    )
    .fetch_one(pool)
    .await?
    .unwrap_or(0);
    println!("SELECT COUNT(*) FROM transactions: {}", txn_count);

    let null_product: i64 = sqlx::query_scalar!(
        "SELECT COUNT(*) FROM transactions WHERE owner_id = $1 AND product_id IS NULL",
        owner_id
    )
    .fetch_one(pool)
    .await?
    .unwrap_or(0);
    println!("product_id IS NULL: {}", null_product);

    let settlement: Option<(Option<rust_decimal::Decimal>, Option<rust_decimal::Decimal>, Option<rust_decimal::Decimal>)> = sqlx::query_as(
        "SELECT recognized_expense, deducted_amount, settlement_input FROM v_monthly_settlement WHERE owner_id = $1 AND month = '2026-02-01'"
    )
    .bind(owner_id)
    .fetch_optional(pool)
    .await?;

    if let Some((recog, deducted, input)) = settlement {
        println!("v_monthly_settlement 2026-02:");
        println!("  recognized_expense: {:?}", recog);
        println!("  deducted_amount: {:?}", deducted);
        println!("  settlement_input: {:?}", input);
    } else {
        println!("v_monthly_settlement: 데이터 없음");
    }

    Ok(())
}
