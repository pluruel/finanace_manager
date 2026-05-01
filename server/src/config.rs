use anyhow::{Context, Result};

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub jwt_issuer: String,
    pub jwt_audience: Vec<String>,
    pub jwks_url: String,
    #[allow(dead_code)] // M2/M3: 서비스 간 호출 식별에 사용 예정
    pub service_name: String,
    pub backend_cors_origins: Vec<String>,
    pub port: u16,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let database_url = std::env::var("DATABASE_URL")
            .context("DATABASE_URL must be set")?;

        let jwt_issuer = std::env::var("JWT_ISSUER")
            .unwrap_or_else(|_| "auth-svc".to_string());

        // JWT_AUDIENCE는 JSON 배열 문자열: ["finance-manager"]
        let jwt_audience_raw = std::env::var("JWT_AUDIENCE")
            .unwrap_or_else(|_| r#"["finance-manager"]"#.to_string());
        let jwt_audience: Vec<String> = serde_json::from_str(&jwt_audience_raw)
            .context("JWT_AUDIENCE must be a JSON array of strings")?;

        let jwks_url = std::env::var("JWKS_URL")
            .unwrap_or_else(|_| {
                "https://auth.junodevs.com/auth/.well-known/jwks.json".to_string()
            });

        let service_name = std::env::var("SERVICE_NAME")
            .unwrap_or_else(|_| "finance-manager".to_string());

        // BACKEND_CORS_ORIGINS는 JSON 배열: ["http://localhost:3000"]
        let cors_raw = std::env::var("BACKEND_CORS_ORIGINS")
            .unwrap_or_else(|_| r#"["http://localhost:3000"]"#.to_string());
        let backend_cors_origins: Vec<String> = serde_json::from_str(&cors_raw)
            .context("BACKEND_CORS_ORIGINS must be a JSON array of strings")?;

        let port = std::env::var("PORT")
            .unwrap_or_else(|_| "8000".to_string())
            .parse::<u16>()
            .context("PORT must be a valid port number")?;

        Ok(Self {
            database_url,
            jwt_issuer,
            jwt_audience,
            jwks_url,
            service_name,
            backend_cors_origins,
            port,
        })
    }
}
