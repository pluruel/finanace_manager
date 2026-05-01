use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
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

    #[error("Conflict: {0}")]
    Conflict(String),

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
                return AppError::Conflict(format!("Duplicate record: {}", db_err.message()));
            }
        }
        AppError::Database(e)
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            AppError::Unauthorized => (StatusCode::UNAUTHORIZED, self.to_string()),
            AppError::Forbidden => (StatusCode::FORBIDDEN, self.to_string()),
            AppError::NotFound(msg) => (StatusCode::NOT_FOUND, msg.clone()),
            AppError::Conflict(msg) => (StatusCode::CONFLICT, msg.clone()),
            AppError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
            AppError::NotImplemented => (StatusCode::NOT_IMPLEMENTED, self.to_string()),
            AppError::PayloadTooLarge => (StatusCode::PAYLOAD_TOO_LARGE, self.to_string()),
            AppError::Database(e) => {
                tracing::error!("Database error: {:?}", e);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Database error".to_string(),
                )
            }
            AppError::Internal(e) => {
                tracing::error!("Internal error: {:?}", e);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal server error".to_string(),
                )
            }
        };

        let body = Json(json!({ "detail": message }));
        (status, body).into_response()
    }
}

pub type AppResult<T> = Result<T, AppError>;
