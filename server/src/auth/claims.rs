use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// JWT 페이로드 구조체
/// MSA_INTEGRATION.md의 클레임 레이아웃을 따름
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JwtClaims {
    pub iss: String,
    pub aud: Vec<String>,
    pub sub: String,         // UUID 문자열 (owner_id로 사용)
    pub email: String,
    pub groups: Vec<String>,
    pub iat: i64,
    pub nbf: i64,
    pub exp: i64,
    pub typ: String,         // "access"여야 함
}

/// 핸들러에 주입되는 인증 사용자 정보
/// MSA 계약: DB에는 sub(owner_id)만 저장. email/groups는 메모리에만 보관.
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub sub: Uuid,
    #[allow(dead_code)] // M2/M3: 권한·표시용으로 사용 예정
    pub email: String,
    #[allow(dead_code)] // M2/M3: 그룹 기반 권한 제어 예정
    pub groups: Vec<String>,
}

impl TryFrom<JwtClaims> for AuthUser {
    type Error = anyhow::Error;

    fn try_from(claims: JwtClaims) -> Result<Self, Self::Error> {
        let sub = Uuid::parse_str(&claims.sub)
            .map_err(|e| anyhow::anyhow!("Invalid sub UUID: {}", e))?;
        Ok(AuthUser {
            sub,
            email: claims.email,
            groups: claims.groups,
        })
    }
}
