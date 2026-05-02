use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("Not authenticated")]
    Unauthorized,

    #[allow(dead_code)] // M2/M3: 소유권 검사에서 사용 예정
    #[error("Forbidden")]
    Forbidden,

    #[allow(dead_code)] // M2/M3: 리소스 조회 실패 시 사용 예정
    #[error("Not found: {0}")]
    NotFound(String),

    /// Structured 409 payload. The Value must contain at minimum
    /// `"error"` (machine code) and `"message"` (human-readable) fields.
    #[error("Conflict")]
    Conflict(Value),

    #[error("Bad request: {0}")]
    BadRequest(String),

    #[allow(dead_code)] // M2/M3: 미구현 엔드포인트 stub에서 사용 예정
    #[error("Not implemented")]
    NotImplemented,

    #[error("Payload too large")]
    PayloadTooLarge,

    #[error("Database error: {0}")]
    Database(sqlx::Error),

    #[error("Internal error: {0}")]
    Internal(#[from] anyhow::Error),
}

/// sqlx::Error → AppError 변환
/// SQLSTATE 23505 (unique_violation)은 Conflict(409)로 매핑.
/// 그 외 DB 오류는 Database(500)으로 매핑.
impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        if let sqlx::Error::Database(ref db_err) = e {
            // PostgreSQL SQLSTATE 23505 = unique_violation
            if db_err.code().as_deref() == Some("23505") {
                return AppError::Conflict(json!({
                    "error": "duplicate_record",
                    "message": format!("Duplicate record: {}", db_err.message()),
                }));
            }
        }
        AppError::Database(e)
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        match self {
            AppError::Unauthorized => {
                let body = Json(json!({ "detail": "Not authenticated" }));
                (StatusCode::UNAUTHORIZED, body).into_response()
            }
            AppError::Forbidden => {
                let body = Json(json!({ "detail": "Forbidden" }));
                (StatusCode::FORBIDDEN, body).into_response()
            }
            AppError::NotFound(msg) => {
                let body = Json(json!({ "detail": msg }));
                (StatusCode::NOT_FOUND, body).into_response()
            }
            AppError::Conflict(payload) => {
                // payload is already a structured Value with "error" + "message" fields
                (StatusCode::CONFLICT, Json(payload)).into_response()
            }
            AppError::BadRequest(msg) => {
                let body = Json(json!({ "detail": msg }));
                (StatusCode::BAD_REQUEST, body).into_response()
            }
            AppError::NotImplemented => {
                let body = Json(json!({ "detail": "Not implemented" }));
                (StatusCode::NOT_IMPLEMENTED, body).into_response()
            }
            AppError::PayloadTooLarge => {
                let body = Json(json!({ "detail": "Payload too large" }));
                (StatusCode::PAYLOAD_TOO_LARGE, body).into_response()
            }
            AppError::Database(e) => {
                tracing::error!("Database error: {:?}", e);
                let body = Json(json!({ "detail": "Database error" }));
                (StatusCode::INTERNAL_SERVER_ERROR, body).into_response()
            }
            AppError::Internal(e) => {
                tracing::error!("Internal error: {:?}", e);
                let body = Json(json!({ "detail": "Internal server error" }));
                (StatusCode::INTERNAL_SERVER_ERROR, body).into_response()
            }
        }
    }
}

pub type AppResult<T> = Result<T, AppError>;
