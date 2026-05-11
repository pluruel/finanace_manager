use std::net::SocketAddr;
use std::sync::Arc;

use axum::http::{HeaderName, HeaderValue, Method};
use migration::MigratorTrait;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

mod api;
mod auth;
mod config;
mod db;
mod domain;
mod entity;
mod error;
mod import;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // .env 파일 로드 (로컬 개발용, 실패해도 무시)
    let _ = dotenvy();

    // 트레이싱 초기화
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| {
            "finance_manager=debug,tower_http=debug,axum=debug".into()
        }))
        .with(tracing_subscriber::fmt::layer())
        .init();

    // 설정 로드
    let config = config::Config::from_env()?;
    tracing::info!("Starting finance-manager on port {}", config.port);

    // DB 연결
    let db = db::create_db(&config.database_url).await?;
    tracing::info!("Database connected");

    // sea-orm 마이그레이션 자동 실행
    migration::Migrator::up(&db, None).await?;
    tracing::info!("Migrations applied");

    let db = Arc::new(db);

    // JWKS 클라이언트 초기화 + 부팅 시 prefetch
    let jwks = Arc::new(auth::JwksClient::new(
        config.jwks_url.clone(),
        config.jwt_issuer.clone(),
        config.jwt_audience.clone(),
    ));
    jwks.prefetch().await?;
    tracing::info!("JWKS cache initialized");

    // CORS 설정
    // MSA 계약: 와일드카드(*) 금지 → 명시적 origin 목록
    let cors = {
        let origins: Vec<HeaderValue> = config
            .backend_cors_origins
            .iter()
            .filter_map(|o| o.parse::<HeaderValue>().ok())
            .collect();

        // MSA 계약: allow_credentials(true)와 allow_headers(Any)는 함께 쓸 수 없음
        // 명시적 헤더 목록을 사용
        CorsLayer::new()
            .allow_origin(origins)
            .allow_methods([
                Method::GET,
                Method::POST,
                Method::PUT,
                Method::PATCH,
                Method::DELETE,
                Method::OPTIONS,
            ])
            .allow_headers([
                HeaderName::from_static("authorization"),
                HeaderName::from_static("content-type"),
                HeaderName::from_static("x-requested-with"),
            ])
            .allow_credentials(true)
    };

    // 라우터 구성
    let app = api::router(db, jwks)
        .layer(cors)
        .layer(TraceLayer::new_for_http());

    // 서버 시작
    let addr = SocketAddr::from(([0, 0, 0, 0], config.port));
    tracing::info!("Listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

/// .env 파일 로드 (없어도 무시)
fn dotenvy() -> Option<()> {
    let content = std::fs::read_to_string(".env").ok()?;
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        if let Some((key, val)) = line.split_once('=') {
            // 이미 설정된 환경변수는 덮어쓰지 않음
            if std::env::var(key.trim()).is_err() {
                std::env::set_var(key.trim(), val.trim().trim_matches('"'));
            }
        }
    }
    Some(())
}
