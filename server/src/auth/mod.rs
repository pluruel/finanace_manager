pub mod claims;
pub mod jwks;

use axum::{
    extract::{FromRequestParts, Request, State},
    http::{request::Parts, HeaderMap},
    middleware::Next,
    response::Response,
};
use std::sync::Arc;

pub use claims::AuthUser;
pub use jwks::JwksClient;

use crate::error::AppError;

/// JWT 추출 미들웨어
/// MSA 계약:
/// - Authorization: Bearer <token> 헤더
/// - Cookie: Authorization=Bearer <token> 쿠키 모두 지원
pub async fn auth_middleware(
    State(jwks): State<Arc<JwksClient>>,
    mut request: Request,
    next: Next,
) -> Result<Response, AppError> {
    let headers = request.headers();
    let token = extract_token(headers)
        .ok_or(AppError::Unauthorized)?;

    let user = jwks
        .verify(&token)
        .await
        .map_err(|_| AppError::Unauthorized)?;

    request.extensions_mut().insert(user);
    Ok(next.run(request).await)
}

/// Authorization 헤더 또는 쿠키에서 Bearer 토큰 추출
fn extract_token(headers: &HeaderMap) -> Option<String> {
    // 1순위: Authorization 헤더
    if let Some(auth_header) = headers.get("authorization") {
        if let Ok(auth_str) = auth_header.to_str() {
            if let Some(token) = strip_bearer(auth_str) {
                return Some(token.to_string());
            }
        }
    }

    // 2순위: Cookie: Authorization=Bearer <token>
    if let Some(cookie_header) = headers.get("cookie") {
        if let Ok(cookie_str) = cookie_header.to_str() {
            for cookie in cookie_str.split(';') {
                let cookie = cookie.trim();
                if let Some(value) = cookie.strip_prefix("Authorization=") {
                    if let Some(token) = strip_bearer(value) {
                        return Some(token.to_string());
                    }
                }
            }
        }
    }

    None
}

fn strip_bearer(s: &str) -> Option<&str> {
    let s = s.trim();
    if s.to_ascii_lowercase().starts_with("bearer ") {
        Some(s[7..].trim())
    } else {
        None
    }
}

/// 핸들러에서 Extension<AuthUser>로 꺼내는 extractor
#[derive(Clone, Debug)]
pub struct ExtractUser(pub AuthUser);

#[axum::async_trait]
impl<S> FromRequestParts<S> for ExtractUser
where
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let user = parts
            .extensions
            .get::<AuthUser>()
            .cloned()
            .ok_or(AppError::Unauthorized)?;
        Ok(ExtractUser(user))
    }
}
