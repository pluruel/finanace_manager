use axum::{
    extract::{Multipart, State},
    extract::multipart::MultipartError,
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;

use crate::auth::ExtractUser;
use crate::domain::ImportResult;
use crate::error::{AppError, AppResult};
use crate::import::{
    pipeline::run_pipeline,
    xlsx::{extract_sheet_name, extract_year_month, parse_xlsx},
};

/// axum `MultipartError`를 `AppError`로 변환한다.
///
/// `MultipartError::status()`가 413(PAYLOAD_TOO_LARGE)이면 `AppError::PayloadTooLarge`로,
/// 그 외는 `AppError::BadRequest`로 매핑한다.
/// axum 0.7은 `LengthLimitError`(DefaultBodyLimit 초과) 를 `StreamReadFailed` 내부에
/// 중첩해 전파하며, `status()` 메서드가 이를 올바르게 PAYLOAD_TOO_LARGE로 분류한다.
fn multipart_err_to_app_err(e: MultipartError) -> AppError {
    if e.status() == StatusCode::PAYLOAD_TOO_LARGE {
        AppError::PayloadTooLarge
    } else {
        AppError::BadRequest(format!("Multipart error: {}", e))
    }
}

/// POST /api/import
/// multipart로 .xlsx 1개 수신 (최대 20MB — DefaultBodyLimit::max(20MB) 라우터 레벨 적용)
/// 1. SHA-256 해시 → import_batches 멱등 삽입 (트랜잭션 내, 중복 409)
/// 2. "M월" 시트 파싱
/// 3. 단일 트랜잭션 내에서: raw 저장 + alias 매핑 + transactions 생성 + 합계 무결성 검증
/// 4. tx.commit() — 실패 시 자동 rollback → file_hash 미점유 → 재시도 가능
pub async fn handle_import(
    State(pool): State<Arc<PgPool>>,
    ExtractUser(user): ExtractUser,
    mut multipart: Multipart,
) -> AppResult<impl IntoResponse> {
    // multipart에서 파일 추출
    let mut file_bytes: Option<Vec<u8>> = None;
    let mut file_name: String = "unknown.xlsx".to_string();

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(multipart_err_to_app_err)?
    {
        let name = field.name().unwrap_or("").to_string();
        if name == "file" || name == "xlsx" || file_bytes.is_none() {
            if let Some(fname) = field.file_name() {
                file_name = fname.to_string();
            }
            let data = field
                .bytes()
                .await
                .map_err(multipart_err_to_app_err)?;
            file_bytes = Some(data.to_vec());
        }
    }

    let bytes = file_bytes.ok_or_else(|| AppError::BadRequest("No file uploaded".to_string()))?;

    if bytes.is_empty() {
        return Err(AppError::BadRequest("Empty file".to_string()));
    }

    // SHA-256 계산
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let hash_bytes = hasher.finalize();
    let hash_vec = hash_bytes.to_vec();

    // 파일명에서 year, month 추출
    let (year, month) = extract_year_month(&file_name)
        .ok_or_else(|| {
            AppError::BadRequest(format!(
                "Cannot extract year/month from filename: {}. Expected format: 'YYYY년 MM월.xlsx'",
                file_name
            ))
        })?;

    // 시트명 추출
    let sheet_name = extract_sheet_name(&file_name)
        .ok_or_else(|| {
            AppError::BadRequest(format!(
                "Cannot extract sheet name from filename: {}",
                file_name
            ))
        })?;

    // xlsx 파싱 (트랜잭션 시작 전 — CPU 작업, DB 불필요)
    let raw_rows = parse_xlsx(&bytes, &sheet_name)
        .map_err(|e| AppError::BadRequest(format!("Failed to parse xlsx: {}", e)))?;

    let row_count = raw_rows.len() as i32;
    let owner_id = user.sub;

    // 단일 트랜잭션 시작
    // import_batches INSERT → raw 저장 → alias 매핑 → transactions 생성 → 무결성 검증 모두 같은 tx.
    // 어느 단계에서든 실패하면 tx drop 시 자동 rollback → file_hash 미점유 → 사용자 재시도 가능.
    let mut tx = pool.begin().await?;

    // import_batches 멱등 삽입 (트랜잭션 내)
    // RETURNING이 None이면 이미 존재 → rollback 후 409
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

    let batch_id: Uuid = match batch_id {
        Some(id) => id,
        None => {
            // tx 드롭 → 자동 rollback (실제로 아무것도 삽입된 게 없음)
            return Err(AppError::Conflict(serde_json::json!({
                "error": "duplicate_import",
                "message": "This file has already been imported (same SHA-256 hash).",
            })));
        }
    };

    // 파이프라인 실행 (같은 트랜잭션)
    let (transactions_inserted, integrity_warnings, unresolved_aliases) =
        run_pipeline(&mut *tx, owner_id, batch_id, raw_rows)
            .await
            .map_err(|e| AppError::Internal(e))?;

    // 모든 단계 성공 → 커밋
    tx.commit().await?;

    let result = ImportResult {
        batch_id,
        year,
        month,
        row_count,
        transactions_inserted,
        integrity_warnings,
        unresolved_aliases,
    };

    Ok((StatusCode::CREATED, Json(result)))
}
