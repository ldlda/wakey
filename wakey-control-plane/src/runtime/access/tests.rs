use super::*;
use axum::body::Body;
use axum::http::{HeaderValue, Request};
use jsonwebtoken::jwk::Jwk;
use jsonwebtoken::{EncodingKey, Header, encode};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::RwLock;
use tower::ServiceExt;

// Public test-only private key, never used as a deployment credential.
const TEST_KEY: &[u8] = include_bytes!("test-key.pem");
const ISSUER: &str = "https://access.example";
const AUDIENCE: &str = "wakey-test";

fn config() -> AccessConfig {
    AccessConfig {
        issuer: ISSUER.into(),
        audience: AUDIENCE.into(),
        jwks_url: None,
    }
}

fn encoding_key() -> EncodingKey {
    EncodingKey::from_rsa_pem(TEST_KEY).unwrap()
}

fn jwks(kid: &str) -> Value {
    let mut jwk = Jwk::from_encoding_key(&encoding_key(), Algorithm::RS256).unwrap();
    jwk.common.key_id = Some(kid.into());
    json!({"keys": [jwk]})
}

fn claims() -> Value {
    // Fixed dates make boundary tests independent of scheduling delays.
    json!({"iss": ISSUER, "aud": [AUDIENCE], "exp": 4102444800u64})
}

fn token(kid: &str, claims: &Value) -> String {
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some(kid.into());
    encode(&header, claims, &encoding_key()).unwrap()
}

async fn cached() -> Arc<Verifier> {
    let verifier = Arc::new(Verifier::new(&config()).unwrap());
    let mut cache = verifier.cache.lock().await;
    cache.keys = parse_keys(&serde_json::to_vec(&jwks("one")).unwrap()).unwrap();
    cache.policy = Some(test_policy(
        &verifier,
        "public, max-age=14400, must-revalidate",
        None,
    ));
    cache.attempted_at = Some(Instant::now());
    drop(cache);
    verifier
}

fn test_policy(verifier: &Verifier, cache_control: &str, etag: Option<&str>) -> CachePolicy {
    let request = verifier
        .client
        .get(verifier.jwks_url.clone())
        .build()
        .unwrap();
    let mut response = axum::http::Response::builder().header("cache-control", cache_control);
    if let Some(etag) = etag {
        response = response.header("etag", etag);
    }
    CachePolicy::new_options(
        &request,
        &response.body(()).unwrap(),
        SystemTime::now(),
        CACHE_OPTIONS,
    )
}

async fn status(app: Router, assertion: Option<&str>) -> StatusCode {
    let mut request = Request::builder().uri("/internal/auth/verify");
    if let Some(assertion) = assertion {
        request = request.header("cf-access-jwt-assertion", assertion);
    }
    app.oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap()
        .status()
}

#[tokio::test]
async fn oracle_accepts_only_valid_application_tokens() {
    let verifier = cached().await;
    let app = verifier_routes(Some(verifier));
    assert_eq!(
        status(app.clone(), Some(&token("one", &claims()))).await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(status(app.clone(), None).await, StatusCode::UNAUTHORIZED);
    assert_eq!(
        status(app.clone(), Some("wakey-invalid-test")).await,
        StatusCode::UNAUTHORIZED
    );
    for (field, value) in [
        ("iss", json!("https://other.example")),
        ("aud", json!(["bypass-app"])),
        ("exp", json!(1)),
        ("nbf", json!(4102444800u64)),
    ] {
        let mut invalid = claims();
        invalid[field] = value;
        assert_eq!(
            status(app.clone(), Some(&token("one", &invalid))).await,
            StatusCode::UNAUTHORIZED,
            "{field}"
        );
    }
    for field in ["iss", "aud", "exp"] {
        let mut invalid = claims();
        invalid.as_object_mut().unwrap().remove(field);
        assert_eq!(
            status(app.clone(), Some(&token("one", &invalid))).await,
            StatusCode::UNAUTHORIZED,
            "missing {field}"
        );
    }
    let mut forged = token("one", &claims()).into_bytes();
    let signature_start = forged.iter().rposition(|byte| *byte == b'.').unwrap() + 1;
    forged[signature_start] = if forged[signature_start] == b'A' {
        b'B'
    } else {
        b'A'
    };
    assert_eq!(
        status(app.clone(), Some(std::str::from_utf8(&forged).unwrap())).await,
        StatusCode::UNAUTHORIZED
    );
    let mut header = Header::new(Algorithm::HS256);
    header.kid = Some("one".into());
    let hmac = encode(
        &header,
        &claims(),
        &EncodingKey::from_secret(b"not-an-rsa-key"),
    )
    .unwrap();
    assert_eq!(
        status(app.clone(), Some(&hmac)).await,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        status(app.clone(), Some(&token("unknown", &claims()))).await,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        status(app.clone(), Some(&"a".repeat(MAX_TOKEN_BYTES + 1))).await,
        StatusCode::UNAUTHORIZED
    );
    let mut request = Request::builder()
        .uri("/internal/auth/verify")
        .body(Body::empty())
        .unwrap();
    let valid = token("one", &claims());
    request.headers_mut().append(
        "cf-access-jwt-assertion",
        HeaderValue::from_str(&valid).unwrap(),
    );
    request
        .headers_mut()
        .append("cf-access-jwt-assertion", HeaderValue::from_static("fake"));
    assert_eq!(
        app.oneshot(request).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        status(verifier_routes(None), Some(&valid)).await,
        StatusCode::UNAUTHORIZED
    );
}

#[test]
fn trusted_configuration_rejects_unsafe_urls_and_non_loopback_bind() {
    assert_eq!(
        Verifier::new(&config()).unwrap().jwks_url.as_str(),
        "https://access.example/cdn-cgi/access/certs"
    );
    let mut explicit = config();
    explicit.jwks_url = Some("https://keys.example/custom".into());
    assert_eq!(
        Verifier::new(&explicit).unwrap().jwks_url.as_str(),
        "https://keys.example/custom"
    );
    for url in [
        "http://access.example",
        "https://user:pass@access.example",
        "https://access.example?q=1",
        "https://access.example/#fragment",
        "not a url",
    ] {
        assert!(trusted_url(url).is_err(), "{url}");
    }
    let mut invalid = config();
    invalid.audience.clear();
    assert!(Verifier::new(&invalid).is_err());
    invalid = config();
    invalid.issuer.push_str("/wrong-path");
    assert!(Verifier::new(&invalid).is_err());
    assert!(routes(Some(&config()), "0.0.0.0:8080".parse().unwrap()).is_err());
    assert!(routes(Some(&config()), "[::1]:8080".parse().unwrap()).is_ok());
}

#[test]
fn signing_key_set_rejects_duplicates_and_unusable_keys() {
    let key = jwks("one")["keys"][0].clone();
    assert!(parse_keys(br#"{"keys": []}"#).unwrap().is_empty());
    for set in [
        json!({"keys": [key.clone(), key.clone()]}),
        json!({"keys": vec![key.clone(); MAX_KEYS + 1]}),
    ] {
        assert!(parse_keys(&serde_json::to_vec(&set).unwrap()).is_err());
    }
    for (field, value) in [
        ("use", json!("enc")),
        ("alg", json!("RS512")),
        ("key_ops", json!(["encrypt"])),
    ] {
        let mut invalid = key.clone();
        invalid[field] = value;
        assert!(
            parse_keys(&serde_json::to_vec(&json!({"keys": [invalid]})).unwrap())
                .unwrap()
                .is_empty()
        );
    }
}

#[tokio::test]
async fn key_fetch_is_single_flight_rotates_and_fails_closed() {
    let hits = Arc::new(AtomicUsize::new(0));
    let reply = Arc::new(RwLock::new((
        StatusCode::OK,
        serde_json::to_string(&jwks("one")).unwrap(),
    )));
    let app = Router::new().route(
        "/keys",
        get({
            let hits = hits.clone();
            let reply = reply.clone();
            move || {
                let hits = hits.clone();
                let reply = reply.clone();
                async move {
                    hits.fetch_add(1, Ordering::Relaxed);
                    let (status, body) = reply.read().await.clone();
                    (
                        status,
                        [("cache-control", "public, max-age=14400, must-revalidate")],
                        body,
                    )
                }
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let mut verifier = Verifier::new(&config()).unwrap();
    // HTTP is allowed only in this in-process fixture, never in loaded config.
    verifier.jwks_url = Url::parse(&format!("http://{address}/keys")).unwrap();
    let verifier = Arc::new(verifier);
    let valid = token("one", &claims());
    let (first, second) = tokio::join!(verifier.validate(&valid), verifier.validate(&valid));
    assert!(first.is_ok() && second.is_ok());
    assert_eq!(hits.load(Ordering::Relaxed), 1);
    for kid in ["random-1", "random-2"] {
        assert_eq!(
            verifier.validate(&token(kid, &claims())).await,
            Err(VerifyError::Invalid)
        );
    }
    assert_eq!(hits.load(Ordering::Relaxed), 1);
    *reply.write().await = (StatusCode::OK, serde_json::to_string(&jwks("two")).unwrap());
    verifier.cache.lock().await.attempted_at = Some(Instant::now() - REFRESH_COOLDOWN);
    assert!(verifier.validate(&token("two", &claims())).await.is_ok());
    assert_eq!(verifier.validate(&valid).await, Err(VerifyError::Invalid));
    assert_eq!(hits.load(Ordering::Relaxed), 2);
    // A failed refresh cannot authorize unknown keys or expired cached keys.
    *reply.write().await = (StatusCode::SERVICE_UNAVAILABLE, "unavailable".into());
    verifier.cache.lock().await.attempted_at = Some(Instant::now() - REFRESH_COOLDOWN);
    assert_eq!(
        verifier.validate(&valid).await,
        Err(VerifyError::Unavailable)
    );
    assert!(verifier.validate(&token("two", &claims())).await.is_ok());
    verifier.cache.lock().await.policy =
        Some(test_policy(&verifier, "max-age=0, must-revalidate", None));
    assert_eq!(
        verifier.validate(&token("two", &claims())).await,
        Err(VerifyError::Unavailable)
    );
    assert_eq!(hits.load(Ordering::Relaxed), 3);
    // Oversized and malformed responses also fail closed, with bounded retries.
    for body in ["x".repeat(MAX_JWKS_BYTES + 1), "not json".into()] {
        *reply.write().await = (StatusCode::OK, body);
        verifier.cache.lock().await.attempted_at = Some(Instant::now() - REFRESH_COOLDOWN);
        assert_eq!(
            verifier.validate(&valid).await,
            Err(VerifyError::Unavailable)
        );
    }
    assert_eq!(
        status(verifier_routes(Some(verifier.clone())), Some(&valid)).await,
        StatusCode::SERVICE_UNAVAILABLE
    );
    // A successful empty set must revoke previously cached keys immediately.
    *reply.write().await = (StatusCode::OK, r#"{"keys": []}"#.into());
    verifier.cache.lock().await.attempted_at = Some(Instant::now() - REFRESH_COOLDOWN);
    assert_eq!(
        verifier.validate(&token("two", &claims())).await,
        Err(VerifyError::Invalid)
    );
    assert!(verifier.cache.lock().await.keys.is_empty());
    server.abort();
    let _ = server.await;
}

struct KeyOrigin {
    verifier: Arc<Verifier>,
    reply: Arc<RwLock<(StatusCode, HeaderMap, String)>>,
    requests: Arc<Mutex<Vec<HeaderMap>>>,
    server: tokio::task::JoinHandle<()>,
}

impl KeyOrigin {
    async fn new(headers: &[(&str, &str)]) -> Self {
        let reply = Arc::new(RwLock::new((
            StatusCode::OK,
            HeaderMap::new(),
            String::new(),
        )));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let app = Router::new().route(
            "/keys",
            get({
                let reply = reply.clone();
                let requests = requests.clone();
                move |headers: HeaderMap| {
                    let reply = reply.clone();
                    let requests = requests.clone();
                    async move {
                        requests.lock().await.push(headers);
                        reply.read().await.clone()
                    }
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let mut verifier = Verifier::new(&config()).unwrap();
        verifier.jwks_url = Url::parse(&format!("http://{address}/keys")).unwrap();
        let origin = Self {
            verifier: Arc::new(verifier),
            reply,
            requests,
            server,
        };
        origin
            .respond(
                StatusCode::OK,
                serde_json::to_string(&jwks("one")).unwrap(),
                headers,
            )
            .await;
        origin
    }

    async fn respond(&self, status: StatusCode, body: String, headers: &[(&str, &str)]) {
        let mut response = axum::http::Response::builder().status(status);
        for (name, value) in headers {
            response = response.header(*name, *value);
        }
        let (parts, body) = response.body(body).unwrap().into_parts();
        *self.reply.write().await = (status, parts.headers, body);
    }

    async fn allow_refresh(&self) {
        self.verifier.cache.lock().await.attempted_at = None;
    }

    async fn stop(self) {
        self.server.abort();
        let _ = self.server.await;
    }
}

#[tokio::test]
async fn http_cache_reuses_fresh_keys_revalidates_etags_and_forces_rotation() {
    let origin = KeyOrigin::new(&[
        (
            "cache-control",
            "public, max-age=14400, must-revalidate, no-transform",
        ),
        ("etag", "\"v1\""),
    ])
    .await;
    let one = token("one", &claims());
    for _ in 0..2 {
        assert!(origin.verifier.validate(&one).await.is_ok());
    }
    assert_eq!(origin.requests.lock().await.len(), 1);
    // Expire the policy directly; no wall-clock sleeps or delayed retries.
    origin.verifier.cache.lock().await.policy = Some(test_policy(
        &origin.verifier,
        "max-age=0, must-revalidate",
        Some("\"v1\""),
    ));
    origin.allow_refresh().await;
    origin
        .respond(
            StatusCode::NOT_MODIFIED,
            String::new(),
            &[
                ("etag", "\"v1\""),
                ("cache-control", "public, max-age=14400, must-revalidate"),
            ],
        )
        .await;
    for _ in 0..2 {
        assert!(origin.verifier.validate(&one).await.is_ok());
    }
    assert_eq!(origin.requests.lock().await.len(), 2);
    assert_eq!(origin.requests.lock().await[1]["if-none-match"], "\"v1\"");
    // An unknown kid forces revalidation even though the HTTP entry is fresh.
    origin.allow_refresh().await;
    origin
        .respond(
            StatusCode::OK,
            serde_json::to_string(&jwks("two")).unwrap(),
            &[
                ("etag", "\"v2\""),
                ("cache-control", "public, max-age=14400, must-revalidate"),
            ],
        )
        .await;
    assert!(
        origin
            .verifier
            .validate(&token("two", &claims()))
            .await
            .is_ok()
    );
    assert_eq!(origin.requests.lock().await.len(), 3);
    assert_eq!(origin.requests.lock().await[2]["cache-control"], "no-cache");
    assert_eq!(origin.requests.lock().await[2]["if-none-match"], "\"v1\"");
    assert_eq!(
        origin.verifier.validate(&one).await,
        Err(VerifyError::Invalid)
    );
    // A mismatched 304 must not renew trust in the stored keys.
    origin.verifier.cache.lock().await.policy = Some(test_policy(
        &origin.verifier,
        "max-age=0, must-revalidate",
        Some("\"v2\""),
    ));
    origin.allow_refresh().await;
    origin
        .respond(
            StatusCode::NOT_MODIFIED,
            String::new(),
            &[("etag", "\"wrong\""), ("cache-control", "max-age=14400")],
        )
        .await;
    assert_eq!(
        origin.verifier.validate(&token("two", &claims())).await,
        Err(VerifyError::Unavailable)
    );
    origin.stop().await;
}

#[tokio::test]
async fn http_cache_honors_age_no_cache_and_no_store() {
    for (cache_control, age, stores) in [
        ("no-store", "0", false),
        ("no-cache", "0", true),
        ("public, max-age=14400, must-revalidate", "14401", true),
        ("", "0", true),
    ] {
        let origin = KeyOrigin::new(&[("cache-control", cache_control), ("age", age)]).await;
        let one = token("one", &claims());
        assert!(origin.verifier.validate(&one).await.is_ok());
        assert_eq!(origin.verifier.cache.lock().await.policy.is_some(), stores);
        if !stores {
            assert!(origin.verifier.cache.lock().await.keys.is_empty());
        }
        // A successful origin response can serve the requesting caller, but
        // an HTTP-stale/non-storable response cannot authorize later requests.
        assert_eq!(
            origin.verifier.validate(&one).await,
            Err(VerifyError::Unavailable)
        );
        assert_eq!(origin.requests.lock().await.len(), 1);
        origin.allow_refresh().await;
        assert!(origin.verifier.validate(&one).await.is_ok());
        assert_eq!(origin.requests.lock().await.len(), 2);
        origin.stop().await;
    }
}

#[tokio::test]
async fn http_cache_revalidates_last_modified_and_rejects_unsolicited_304() {
    let modified = "Wed, 01 Jan 2025 00:00:00 GMT";
    let origin = KeyOrigin::new(&[
        ("cache-control", "max-age=0, must-revalidate"),
        ("last-modified", modified),
    ])
    .await;
    let one = token("one", &claims());
    assert!(origin.verifier.validate(&one).await.is_ok());
    origin.allow_refresh().await;
    origin
        .respond(
            StatusCode::NOT_MODIFIED,
            String::new(),
            &[
                ("last-modified", modified),
                ("cache-control", "max-age=14400"),
            ],
        )
        .await;
    assert!(origin.verifier.validate(&one).await.is_ok());
    assert_eq!(
        origin.requests.lock().await[1]["if-modified-since"],
        modified
    );
    origin.stop().await;

    let origin = KeyOrigin::new(&[]).await;
    origin
        .respond(StatusCode::NOT_MODIFIED, String::new(), &[])
        .await;
    assert_eq!(
        origin.verifier.validate(&one).await,
        Err(VerifyError::Unavailable)
    );
    assert!(origin.verifier.cache.lock().await.policy.is_none());
    origin.stop().await;
}

#[tokio::test]
async fn verifier_routes_do_not_gate_agent_or_control_handlers() {
    let dir = std::env::temp_dir().join(format!("wakey-access-routes-{}", uuid::Uuid::new_v4()));
    let store = crate::state::Store::load_or_init(
        &dir.join("state.sqlite3"),
        vec![],
        Duration::from_secs(60),
    )
    .await
    .unwrap();
    let state = AppState {
        store: Arc::new(store),
        sessions: Arc::new(RwLock::new(HashMap::new())),
        pending: Arc::new(Mutex::new(HashMap::new())),
        public_url: "http://localhost".into(),
        command_timeout: Duration::from_secs(30),
        enroll_token_ttl: Duration::from_secs(60),
        terminals: super::super::terminals::TerminalRegistry::new(),
        alerts: crate::api::AlertsHub::new(),
        fleet: Arc::new(crate::api::FleetCache::default()),
    };
    let app = Router::new()
        .merge(routes(Some(&config()), "127.0.0.1:8080".parse().unwrap()).unwrap())
        .merge(super::super::public_api_routes(dir.clone()))
        .merge(super::super::control_api_routes())
        .with_state(state);
    for (path, expected) in [
        ("/healthz", StatusCode::OK),
        ("/api/v1/control/agents", StatusCode::OK),
        ("/internal/auth/verify", StatusCode::UNAUTHORIZED),
    ] {
        let response = app
            .clone()
            .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), expected, "{path}");
    }
    drop(app);
    std::fs::remove_dir_all(dir).unwrap();
}
