//! Origin authentication oracle for Caddy, not middleware on the control API.
//! Enforcement belongs to the proxy; the daemon must remain loopback-only.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result, ensure};
use axum::{
    Extension, Router,
    http::{HeaderMap, StatusCode},
    routing::get,
};
use http_cache_semantics::{AfterResponse, BeforeRequest, CacheOptions, CachePolicy};
use jsonwebtoken::jwk::{AlgorithmParameters, JwkSet, KeyAlgorithm, KeyOperations, PublicKeyUse};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use reqwest::{Client, Url};
use tokio::sync::Mutex;
use tokio::time::Instant;

use super::AppState;
use crate::config::AccessConfig;

const REFRESH_COOLDOWN: Duration = Duration::from_secs(60);
const FETCH_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_JWKS_BYTES: usize = 256 * 1024;
const MAX_TOKEN_BYTES: usize = 16 * 1024;
const MAX_KEYS: usize = 64;

// This is a private, single-resource cache. Do not invent a freshness lifetime
// when the issuer supplies none, or override its no-cache/no-store directives.
const CACHE_OPTIONS: CacheOptions = CacheOptions {
    shared: false,
    cache_heuristic: 0.0,
    immutable_min_time_to_live: Duration::ZERO,
    ignore_cargo_cult: false,
};

/// Absent configuration deliberately leaves the oracle denying every request.
pub(super) fn routes(config: Option<&AccessConfig>, bind: SocketAddr) -> Result<Router<AppState>> {
    let verifier = config
        .map(|config| {
            ensure!(
                bind.ip().is_loopback(),
                "Access verification requires a loopback bind; Caddy owns enforcement"
            );
            Verifier::new(config).map(Arc::new)
        })
        .transpose()?;
    Ok(verifier_routes(verifier))
}

fn verifier_routes<S: Clone + Send + Sync + 'static>(verifier: Option<Arc<Verifier>>) -> Router<S> {
    Router::new()
        .route("/internal/auth/verify", get(verify))
        .layer(Extension(verifier))
}

async fn verify(
    Extension(verifier): Extension<Option<Arc<Verifier>>>,
    headers: HeaderMap,
) -> StatusCode {
    let Some(verifier) = verifier else {
        return StatusCode::UNAUTHORIZED;
    };
    let mut values = headers.get_all("cf-access-jwt-assertion").iter();
    let Some(token) = values.next().and_then(|value| value.to_str().ok()) else {
        return StatusCode::UNAUTHORIZED;
    };
    if values.next().is_some() {
        return StatusCode::UNAUTHORIZED;
    }
    match verifier.validate(token).await {
        Ok(()) => StatusCode::NO_CONTENT,
        Err(VerifyError::Invalid) => StatusCode::UNAUTHORIZED,
        Err(VerifyError::Unavailable) => StatusCode::SERVICE_UNAVAILABLE,
    }
}

struct Verifier {
    client: Client,
    jwks_url: Url,
    validation: Validation,
    cache: Mutex<KeyCache>,
}

#[derive(Default)]
struct KeyCache {
    keys: HashMap<String, DecodingKey>,
    policy: Option<CachePolicy>,
    attempted_at: Option<Instant>,
    last_fetch_failed: bool,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
enum VerifyError {
    #[error("invalid Access assertion")]
    Invalid,
    #[error("Access signing keys unavailable")]
    Unavailable,
}

impl Verifier {
    fn new(config: &AccessConfig) -> Result<Self> {
        let issuer = trusted_url(&config.issuer)?;
        ensure!(
            issuer.path() == "/" && !config.issuer.ends_with('/'),
            "Access issuer must be an HTTPS origin without a trailing slash"
        );
        ensure!(
            !config.audience.trim().is_empty(),
            "Access audience must not be empty"
        );
        let jwks_url = match &config.jwks_url {
            Some(url) => trusted_url(url)?,
            None => issuer.join("/cdn-cgi/access/certs")?,
        };
        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_issuer(&[&config.issuer]);
        validation.set_audience(&[&config.audience]);
        validation.set_required_spec_claims(&["exp", "iss", "aud"]);
        validation.validate_nbf = true;
        validation.leeway = 0;
        Ok(Self {
            client: Client::builder()
                .timeout(FETCH_TIMEOUT)
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
            jwks_url,
            validation,
            cache: Mutex::new(KeyCache::default()),
        })
    }

    async fn validate(&self, token: &str) -> Result<(), VerifyError> {
        if token.len() > MAX_TOKEN_BYTES {
            return Err(VerifyError::Invalid);
        }
        // Only kid selects a cached key. Token-supplied URLs never drive I/O.
        let header = decode_header(token).map_err(|_| VerifyError::Invalid)?;
        if header.alg != Algorithm::RS256 {
            return Err(VerifyError::Invalid);
        }
        let kid = header
            .kid
            .filter(|kid| !kid.is_empty() && kid.len() <= 256)
            .ok_or(VerifyError::Invalid)?;
        let key = self.key(&kid).await?;
        decode::<serde_json::Value>(token, &key, &self.validation)
            .map(|_| ())
            .map_err(|_| VerifyError::Invalid)
    }

    async fn key(&self, kid: &str) -> Result<DecodingKey, VerifyError> {
        // Single-flight refresh with a bounded HTTP fetch. The cooldown is
        // global, not an attacker-controlled per-kid cache, including outages.
        let mut cache = self.cache.lock().await;
        let mut request = self
            .client
            .get(self.jwks_url.clone())
            .build()
            .map_err(|_| VerifyError::Unavailable)?;
        let now = Instant::now();
        let fresh = cache.policy.as_ref().is_some_and(|policy| {
            matches!(
                policy.before_request(&request, SystemTime::now()),
                BeforeRequest::Fresh(_)
            )
        });
        if fresh && let Some(key) = cache.keys.get(kid) {
            return Ok(key.clone());
        }
        if cache
            .attempted_at
            .is_none_or(|at| now.duration_since(at) >= REFRESH_COOLDOWN)
        {
            cache.attempted_at = Some(now);
            // A missing kid must bypass a still-fresh response cache to notice
            // rotation. The HTTP library adds ETag/Last-Modified validators.
            request.headers_mut().insert(
                reqwest::header::CACHE_CONTROL,
                reqwest::header::HeaderValue::from_static("no-cache"),
            );
            if let Some(policy) = &cache.policy
                && let BeforeRequest::Stale {
                    request: conditional,
                    ..
                } = policy.before_request(&request, SystemTime::now())
            {
                *request.headers_mut() = conditional.headers;
            }
            match self.fetch_keys(&request, &cache).await {
                Ok((keys, policy)) => {
                    let selected = keys.get(kid).cloned().ok_or(VerifyError::Invalid);
                    if policy.is_storable() {
                        cache.keys = keys;
                        cache.policy = Some(policy);
                    } else {
                        // no-store permits this response's use for this request,
                        // but neither keys nor HTTP metadata may be retained.
                        cache.keys.clear();
                        cache.policy = None;
                    }
                    cache.last_fetch_failed = false;
                    return selected;
                }
                Err(_) => {
                    // Never include upstream bodies, tokens or claims in logs.
                    tracing::warn!(
                        "Access signing-key refresh failed; verification fails closed when no fresh matching key is available"
                    );
                    cache.last_fetch_failed = true;
                }
            }
        }
        if !fresh || cache.last_fetch_failed {
            Err(VerifyError::Unavailable)
        } else {
            Err(VerifyError::Invalid)
        }
    }

    async fn fetch_keys(
        &self,
        request: &reqwest::Request,
        cached: &KeyCache,
    ) -> Result<(HashMap<String, DecodingKey>, CachePolicy)> {
        let mut response = self
            .client
            .execute(
                request
                    .try_clone()
                    .context("JWKS request cannot be cloned")?,
            )
            .await?
            .error_for_status()?;
        let received_at = SystemTime::now();
        if response.status() == StatusCode::NOT_MODIFIED {
            ensure!(
                request
                    .headers()
                    .contains_key(reqwest::header::IF_NONE_MATCH)
                    || request
                        .headers()
                        .contains_key(reqwest::header::IF_MODIFIED_SINCE),
                "unsolicited JWKS 304"
            );
            let policy = cached
                .policy
                .as_ref()
                .context("JWKS 304 without stored keys")?;
            return match policy.after_response(request, &response, received_at) {
                AfterResponse::NotModified(policy, _) => Ok((cached.keys.clone(), policy)),
                AfterResponse::Modified(_, _) => {
                    anyhow::bail!("JWKS 304 did not match stored validators")
                }
            };
        }
        ensure!(
            response.status() == StatusCode::OK,
            "JWKS must return 200 or a matching 304"
        );
        let policy = CachePolicy::new_options(request, &response, received_at, CACHE_OPTIONS);
        ensure!(
            response
                .content_length()
                .is_none_or(|len| len <= MAX_JWKS_BYTES as u64),
            "JWKS too large"
        );
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            ensure!(chunk.len() <= MAX_JWKS_BYTES - body.len(), "JWKS too large");
            body.extend_from_slice(&chunk);
        }
        Ok((parse_keys(&body)?, policy))
    }
}

fn trusted_url(value: &str) -> Result<Url> {
    let url = Url::parse(value).context("invalid Access URL")?;
    ensure!(
        url.scheme() == "https"
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none(),
        "Access URLs require HTTPS without credentials, query or fragment"
    );
    Ok(url)
}

fn parse_keys(body: &[u8]) -> Result<HashMap<String, DecodingKey>> {
    let jwks: JwkSet = serde_json::from_slice(body)?;
    ensure!(jwks.keys.len() <= MAX_KEYS, "too many signing keys");
    let mut keys = HashMap::new();
    for jwk in jwks.keys {
        if !matches!(jwk.algorithm, AlgorithmParameters::RSA(_))
            || jwk
                .common
                .key_algorithm
                .is_some_and(|alg| alg != KeyAlgorithm::RS256)
            || jwk
                .common
                .public_key_use
                .as_ref()
                .is_some_and(|usage| *usage != PublicKeyUse::Signature)
            || jwk
                .common
                .key_operations
                .as_ref()
                .is_some_and(|ops| !ops.contains(&KeyOperations::Verify))
        {
            continue;
        }
        let Some(kid) = jwk
            .common
            .key_id
            .as_ref()
            .filter(|kid| !kid.is_empty() && kid.len() <= 256)
        else {
            continue;
        };
        let key = DecodingKey::from_jwk(&jwk)?;
        ensure!(
            keys.insert(kid.clone(), key).is_none(),
            "duplicate signing key ID"
        );
    }
    // A valid empty set revokes every cached key. Do not treat it as a fetch
    // failure and preserve keys that the issuer has deliberately removed.
    Ok(keys)
}

#[cfg(test)]
mod tests;
