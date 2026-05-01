use anyhow::{Context, Result};
use jsonwebtoken::{
    decode,
    jwk::{AlgorithmParameters, JwkSet},
    Algorithm, DecodingKey, Validation,
};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

use super::claims::{AuthUser, JwtClaims};

const JWKS_CACHE_TTL: Duration = Duration::from_secs(300); // 5분

#[derive(Debug)]
struct JwksCache {
    jwks: JwkSet,
    fetched_at: Instant,
}

/// JWKS 캐시 + 검증 엔진
/// MSA 계약:
/// - 5분 TTL 메모리 캐시
/// - 검증 실패 시 1회 강제 재fetch 후 재시도
/// - kid 검증 비활성 (auth-svc가 kid 없이 발급)
pub struct JwksClient {
    jwks_url: String,
    http: reqwest::Client,
    cache: Arc<RwLock<Option<JwksCache>>>,
    issuer: String,
    audience: Vec<String>,
}

impl JwksClient {
    pub fn new(jwks_url: String, issuer: String, audience: Vec<String>) -> Self {
        let http = reqwest::Client::builder()
            .use_rustls_tls()
            .timeout(Duration::from_secs(10))
            .build()
            .expect("Failed to build HTTP client");

        Self {
            jwks_url,
            http,
            cache: Arc::new(RwLock::new(None)),
            issuer,
            audience,
        }
    }

    /// 부팅 시 최초 JWKS fetch
    pub async fn prefetch(&self) -> Result<()> {
        self.fetch_and_cache().await?;
        tracing::info!("JWKS prefetch complete");
        Ok(())
    }

    async fn fetch_and_cache(&self) -> Result<JwkSet> {
        tracing::debug!("Fetching JWKS from {}", self.jwks_url);
        let resp = self
            .http
            .get(&self.jwks_url)
            .send()
            .await
            .context("Failed to fetch JWKS")?;

        let jwks: JwkSet = resp
            .json()
            .await
            .context("Failed to parse JWKS response")?;

        let mut cache = self.cache.write().await;
        *cache = Some(JwksCache {
            jwks: jwks.clone(),
            fetched_at: Instant::now(),
        });

        Ok(jwks)
    }

    async fn get_jwks(&self) -> Result<JwkSet> {
        {
            let cache = self.cache.read().await;
            if let Some(ref c) = *cache {
                if c.fetched_at.elapsed() < JWKS_CACHE_TTL {
                    return Ok(c.jwks.clone());
                }
            }
        }
        // 캐시 만료 또는 없음 → 재fetch
        self.fetch_and_cache().await
    }

    /// JWT 토큰 검증
    /// MSA 계약: EdDSA, iss=auth-svc, aud 배열에 service 포함, exp 미만료, typ=access
    /// 검증 실패(키 미스매치)면 1회 강제 재fetch 후 재시도
    pub async fn verify(&self, token: &str) -> Result<AuthUser> {
        // 1차 시도
        match self.verify_with_cached_jwks(token).await {
            Ok(user) => Ok(user),
            Err(e) => {
                // 키 관련 오류면 강제 재fetch 후 재시도
                tracing::warn!("JWT verification failed ({}), force-refreshing JWKS", e);
                self.fetch_and_cache().await?;
                self.verify_with_cached_jwks(token).await
            }
        }
    }

    async fn verify_with_cached_jwks(&self, token: &str) -> Result<AuthUser> {
        let jwks = self.get_jwks().await?;

        // kid 없음 → 모든 키 시도
        // auth-svc는 kid 없이 발급하므로 키를 순서대로 시도
        let keys: Vec<DecodingKey> = jwks
            .keys
            .iter()
            .filter_map(|jwk| {
                match &jwk.algorithm {
                    AlgorithmParameters::OctetKeyPair(_params) => {
                        // EdDSA (Ed25519)
                        DecodingKey::from_jwk(jwk).ok()
                    }
                    _ => None,
                }
            })
            .collect();

        if keys.is_empty() {
            // EdDSA 키 없으면 모든 키 시도 (필터 완화)
            let all_keys: Vec<DecodingKey> = jwks
                .keys
                .iter()
                .filter_map(|jwk| DecodingKey::from_jwk(jwk).ok())
                .collect();

            return self.try_verify_with_keys(token, &all_keys).await;
        }

        self.try_verify_with_keys(token, &keys).await
    }

    async fn try_verify_with_keys(&self, token: &str, keys: &[DecodingKey]) -> Result<AuthUser> {
        let mut last_err = anyhow::anyhow!("No keys available");

        for key in keys {
            let mut validation = Validation::new(Algorithm::EdDSA);
            // MSA 계약: aud 배열 containment 검증
            validation.set_audience(&self.audience);
            // MSA 계약: iss 검증
            validation.set_issuer(&[&self.issuer]);
            // exp 검증 (기본 활성)
            // kid 검증 비활성 (kid 없음)

            match decode::<JwtClaims>(token, key, &validation) {
                Ok(token_data) => {
                    let claims = token_data.claims;

                    // MSA 계약: typ=access 확인
                    if claims.typ != "access" {
                        return Err(anyhow::anyhow!(
                            "Invalid token type: expected 'access', got '{}'",
                            claims.typ
                        ));
                    }

                    let user = AuthUser::try_from(claims)?;
                    return Ok(user);
                }
                Err(e) => {
                    last_err = anyhow::anyhow!("JWT decode error: {}", e);
                }
            }
        }

        Err(last_err)
    }
}
