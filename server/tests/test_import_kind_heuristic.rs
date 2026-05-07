//! Importer kind 휴리스틱 통합 테스트.
//!
//! 골든 xlsx (`2026년 02월.xlsx`) 를 import 하여 신규 생성된 카테고리들의
//! `kind` 가 이름 기반 휴리스틱(`급여|수입|회수|환급`)에 따라 income/expense
//! 로 분류되는지 확인한다.

use finance_manager::import::pipeline::run_pipeline;
use finance_manager::import::xlsx::{extract_sheet_name, extract_year_month, parse_xlsx};
use sqlx::PgPool;
use uuid::Uuid;

fn load_golden_bytes() -> Vec<u8> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/2026년_02월.xlsx"
    );
    std::fs::read(path).expect("Failed to read golden xlsx fixture")
}

async fn run_golden_import(pool: &PgPool, owner_id: Uuid) -> anyhow::Result<()> {
    use sha2::{Digest, Sha256};
    let bytes = load_golden_bytes();
    let filename = "2026년 02월.xlsx";
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let hash_vec = hasher.finalize().to_vec();

    let (year, month) = extract_year_month(filename).unwrap();
    let sheet_name = extract_sheet_name(filename).unwrap();
    let raw_rows = parse_xlsx(&bytes, &sheet_name)?;
    let row_count = raw_rows.len() as i32;

    let mut tx = pool.begin().await?;
    let batch_id: Uuid = sqlx::query_scalar!(
        r#"INSERT INTO import_batches (owner_id, file_name, file_hash, year, month, row_count)
           VALUES ($1, $2, $3, $4, $5, $6)
           RETURNING id"#,
        owner_id, filename, hash_vec, year, month, row_count,
    )
    .fetch_one(&mut *tx)
    .await?;

    run_pipeline(&mut *tx, owner_id, batch_id, raw_rows).await?;
    tx.commit().await?;
    Ok(())
}

async fn kind_of(pool: &PgPool, owner_id: Uuid, name: &str) -> Option<String> {
    sqlx::query_scalar!(
        "SELECT kind FROM categories WHERE owner_id = $1 AND name = $2 AND parent_id IS NULL",
        owner_id, name
    )
    .fetch_optional(pool)
    .await
    .unwrap()
}

#[sqlx::test(migrations = "./migrations")]
async fn import_classifies_income_categories_by_name(pool: PgPool) {
    let owner_id = Uuid::new_v4();
    run_golden_import(&pool, owner_id).await.unwrap();

    // 키워드 매치 → income
    assert_eq!(kind_of(&pool, owner_id, "급여").await.as_deref(), Some("income"));
    assert_eq!(kind_of(&pool, owner_id, "회수").await.as_deref(), Some("income"));
    assert_eq!(kind_of(&pool, owner_id, "수입 기타").await.as_deref(), Some("income"));
}

#[sqlx::test(migrations = "./migrations")]
async fn import_keeps_other_categories_as_expense(pool: PgPool) {
    let owner_id = Uuid::new_v4();
    run_golden_import(&pool, owner_id).await.unwrap();

    // 키워드 미매치 → expense
    assert_eq!(kind_of(&pool, owner_id, "차감").await.as_deref(), Some("expense"));
    assert_eq!(kind_of(&pool, owner_id, "외식 아침").await.as_deref(), Some("expense"));
    assert_eq!(kind_of(&pool, owner_id, "병원").await.as_deref(), Some("expense"));
}

#[sqlx::test(migrations = "./migrations")]
async fn upsert_preserves_existing_kind_via_on_conflict(pool: PgPool) {
    // ON CONFLICT DO NOTHING 의 보존성을 SQL 레벨에서 직접 확인.
    let owner_id = Uuid::new_v4();

    sqlx::query!(
        "INSERT INTO categories (owner_id, name, kind) VALUES ($1, '외식', 'income')",
        owner_id
    )
    .execute(&pool)
    .await
    .unwrap();

    let _ = sqlx::query!(
        r#"INSERT INTO categories (owner_id, name, kind, review_state)
           VALUES ($1, '외식', 'expense', 'pending')
           ON CONFLICT (owner_id, name) WHERE parent_id IS NULL DO NOTHING"#,
        owner_id
    )
    .execute(&pool)
    .await
    .unwrap();

    let kind: String = sqlx::query_scalar!(
        "SELECT kind FROM categories WHERE owner_id = $1 AND name = '외식'",
        owner_id
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(kind, "income");
}
