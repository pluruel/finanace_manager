/// M1 검증용 독립 바이너리
/// 실제 JWT 없이 pipeline을 직접 테스트
/// usage: DATABASE_URL=... cargo run --bin test_import
use migration::MigratorTrait;
use sea_orm::{
    ActiveValue::Set, DbErr, EntityTrait, TransactionTrait,
    sea_query::OnConflict,
};
use uuid::Uuid;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("finance_manager=debug")
        .init();

    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://app:app@localhost:5432/finance".to_string());

    let db = finance_manager::db::create_db(&database_url).await?;
    migration::Migrator::up(&db, None).await?;

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
    use finance_manager::entity::{import_batches, prelude::ImportBatches};

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
    let txn = db.begin().await?;

    // import_batches 삽입
    let result = ImportBatches::insert(import_batches::ActiveModel {
        owner_id: Set(owner_id),
        file_name: Set(file_name.clone()),
        file_hash: Set(hash_vec.clone()),
        year: Set(year),
        month: Set(month),
        row_count: Set(row_count),
        ..Default::default()
    })
    .on_conflict(
        OnConflict::columns([
            import_batches::Column::OwnerId,
            import_batches::Column::FileHash,
        ])
        .do_nothing()
        .to_owned(),
    )
    .exec(&txn)
    .await;

    let batch_id = match result {
        Ok(r) => {
            let id = r.last_insert_id;
            println!("New import batch: {}", id);
            id
        }
        Err(DbErr::RecordNotInserted) => {
            println!("Duplicate file! Deleting old data and re-importing...");
            // 트랜잭션 롤백 후 기존 데이터 삭제 후 재임포트
            drop(txn);

            use sea_orm::{ColumnTrait, QueryFilter};

            let existing = ImportBatches::find()
                .filter(import_batches::Column::OwnerId.eq(owner_id))
                .filter(import_batches::Column::FileHash.eq(hash_vec.clone()))
                .one(&db)
                .await?
                .expect("existing batch not found");
            let existing_id = existing.id;

            ImportBatches::delete_by_id(existing_id).exec(&db).await?;

            let txn2 = db.begin().await?;
            let r2 = ImportBatches::insert(import_batches::ActiveModel {
                owner_id: Set(owner_id),
                file_name: Set(file_name.clone()),
                file_hash: Set(hash_vec),
                year: Set(year),
                month: Set(month),
                row_count: Set(row_count),
                ..Default::default()
            })
            .exec(&txn2)
            .await?;
            let new_id = r2.last_insert_id;

            let (transactions_inserted, integrity_warnings, unresolved) =
                run_pipeline(&txn2, owner_id, new_id, raw_rows).await?;

            txn2.commit().await?;

            println!("Re-import batch: {}", new_id);
            print_results(transactions_inserted, &integrity_warnings, &unresolved);
            return verify_db(&db, owner_id).await;
        }
        Err(e) => return Err(e.into()),
    };

    // 파이프라인 실행
    let (transactions_inserted, integrity_warnings, unresolved) =
        run_pipeline(&txn, owner_id, batch_id, raw_rows).await?;

    txn.commit().await?;

    print_results(transactions_inserted, &integrity_warnings, &unresolved);
    verify_db(&db, owner_id).await
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

async fn verify_db(db: &sea_orm::DatabaseConnection, owner_id: uuid::Uuid) -> anyhow::Result<()> {
    use sea_orm::{ConnectionTrait, DatabaseBackend, FromQueryResult, Statement};

    println!("\n=== DB 검증 ===");

    #[derive(FromQueryResult)]
    struct CountRow { count: i64 }

    let txn_count = CountRow::find_by_statement(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT COUNT(*) AS count FROM transactions WHERE owner_id = $1",
        [owner_id.into()],
    ))
    .one(db)
    .await?
    .map(|r| r.count)
    .unwrap_or(0);
    println!("SELECT COUNT(*) FROM transactions: {}", txn_count);

    let null_product = CountRow::find_by_statement(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT COUNT(*) AS count FROM transactions WHERE owner_id = $1 AND product_id IS NULL",
        [owner_id.into()],
    ))
    .one(db)
    .await?
    .map(|r| r.count)
    .unwrap_or(0);
    println!("product_id IS NULL: {}", null_product);

    #[derive(FromQueryResult)]
    struct SettlementRow {
        recognized_expense: Option<rust_decimal::Decimal>,
        deducted_amount: Option<rust_decimal::Decimal>,
        settlement_input: Option<rust_decimal::Decimal>,
    }

    let settlement = SettlementRow::find_by_statement(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT recognized_expense, deducted_amount, settlement_input FROM v_monthly_settlement WHERE owner_id = $1 AND month = '2026-02-01'",
        [owner_id.into()],
    ))
    .one(db)
    .await?;

    if let Some(row) = settlement {
        println!("v_monthly_settlement 2026-02:");
        println!("  recognized_expense: {:?}", row.recognized_expense);
        println!("  deducted_amount: {:?}", row.deducted_amount);
        println!("  settlement_input: {:?}", row.settlement_input);
    } else {
        println!("v_monthly_settlement: 데이터 없음");
    }

    Ok(())
}
