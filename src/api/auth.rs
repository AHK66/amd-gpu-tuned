use std::collections::HashMap;
use std::time::{Duration, Instant};

use argon2::password_hash::{phc::PasswordHash, PasswordHasher, PasswordVerifier};
use argon2::{Argon2, Algorithm, Version};
use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::IntoResponse;
use axum::Json;
use rand::RngCore;
use serde_json::{json, Value};
use tokio::sync::RwLock;

use super::{json_err, ApiError, AppState};
use crate::store::Store;

pub const AUTH_PASSWORD_HASH_KEY: &str = "auth_password_hash";
/// Sliding TTL: each authenticated request resets the expiry (spec §4).
pub const TOKEN_TTL: Duration = Duration::from_secs(30 * 60);

// No Clone derive: tokio::sync::RwLock is !Clone. AuthState is only ever
// held behind Arc<AuthState> (AppState.auth).
pub struct AuthState {
    pub tokens: RwLock<HashMap<String, Instant>>,
}

impl AuthState {
    pub fn new() -> Self {
        Self {
            tokens: RwLock::new(HashMap::new()),
        }
    }

    /// Issue a token, prune expired ones, return (token, ttl_s).
    pub async fn issue(&self) -> (String, u64) {
        let mut map = self.tokens.write().await;
        map.retain(|_, exp| *exp > Instant::now());
        let mut bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut bytes);
        let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        map.insert(token.clone(), Instant::now() + TOKEN_TTL);
        (token, TOKEN_TTL.as_secs())
    }

    /// True if the token is known; slides its expiry on success.
    pub async fn check(&self, token: &str) -> bool {
        let mut map = self.tokens.write().await;
        match map.get(token) {
            Some(exp) if *exp > Instant::now() => {
                map.insert(token.to_string(), Instant::now() + TOKEN_TTL);
                true
            }
            Some(_) => {
                map.remove(token);
                false
            }
            None => false,
        }
    }

    /// Drop all tokens (global lock).
    pub async fn lock(&self) {
        self.tokens.write().await.clear();
    }
}

/// Read the hash fresh from meta (None = auth disabled). Free function —
/// a pure function of the store, so it does not live on AuthState.
pub fn password_hash(store: &Store) -> Option<String> {
    store.get_meta(AUTH_PASSWORD_HASH_KEY).ok().flatten()
}

/// Hash with the spec §4 parameters: argon2id v19, m=19456 KiB, t=2, p=1.
pub fn hash_password(password: &str) -> String {
    let salt = rand::random::<[u8; 16]>();
    let params =
        argon2::Params::new(19456, 2, 1, None).expect("valid argon2 parameters");
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_with_salt(password.as_bytes(), &salt)
        .expect("hashing")
        .to_string()
}

/// Verify against an owned hash string (the `unlock` handler fetches
/// `password_hash(&store)` first and moves the `String` into
/// `spawn_blocking`, since a `&Store` borrow cannot cross that boundary).
/// Unparseable hash ⇒ false (never panics on a corrupt meta row).
pub fn verify_hash(hash: &str, password: &str) -> bool {
    match PasswordHash::new(hash) {
        Ok(h) => Argon2::default().verify_password(password.as_bytes(), &h).is_ok(),
        Err(_) => false,
    }
}

/// `POST /api/auth/unlock` — { "password": "..." } → { "token", "ttl_s" }.
/// 400 if no password is configured; 401 on a bad password (also appends an
/// `auth_unlock_failed` event). The argon2 verify (~50–100 ms) runs in
/// `spawn_blocking` so it does not stall a runtime worker thread.
pub async fn unlock(
    State(st): State<AppState>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let password = body
        .get("password")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let hash = match password_hash(&st.store) {
        Some(h) => h,
        None => {
            return Err(json_err(
                StatusCode::BAD_REQUEST,
                "no password configured",
            ))
        }
    };
    let ok = tokio::task::spawn_blocking(move || verify_hash(&hash, &password))
        .await
        .unwrap_or(false);
    if !ok {
        st.store.append_event("auth_unlock_failed", &json!({}));
        return Err(json_err(StatusCode::UNAUTHORIZED, "wrong password"));
    }
    let (token, ttl_s) = st.auth.issue().await;
    Ok(Json(json!({ "token": token, "ttl_s": ttl_s })))
}

/// `POST /api/auth/lock` — requires a valid Bearer token (enforced by the
/// gate, which rejects before the handler runs); drops all tokens.
pub async fn lock(State(st): State<AppState>) -> Json<Value> {
    st.auth.lock().await;
    Json(json!({ "locked": true }))
}

/// Gate middleware: pass-through when no password hash is stored. Otherwise
/// requires `Authorization: Bearer <valid token>`; else 401, short-circuited
/// without touching the inner handler. Applied to the write subrouter only.
pub async fn gate(
    State(st): State<AppState>,
    req: Request,
    next: Next,
) -> axum::response::Response {
    if password_hash(&st.store).is_some() {
        let token = req
            .headers()
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "));
        let ok = match token {
            Some(t) => st.auth.check(t).await,
            None => false,
        };
        if !ok {
            return json_err(StatusCode::UNAUTHORIZED, "authentication required")
                .into_response();
        }
    }
    next.run(req).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_roundtrip() {
        let h = hash_password("s3cret-pw");
        assert!(h.starts_with("$argon2id$v=19$"));
        assert!(verify_hash(&h, "s3cret-pw"));
        assert!(!verify_hash(&h, "wrong-pw"));
        assert!(!verify_hash("not-a-phc-string", "s3cret-pw"));
    }

    #[tokio::test]
    async fn token_issue_check_prune() {
        let st = AuthState::new();
        let (t1, ttl_s) = st.issue().await;
        assert_eq!(ttl_s, 1800);
        assert_eq!(t1.len(), 64);
        assert!(st.check(&t1).await);
        // An expired entry is rejected and pruned on the next check.
        st.tokens
            .write()
            .await
            .insert("dead".into(), Instant::now() - Duration::from_secs(1));
        assert!(!st.check("dead").await);
        assert!(!st.tokens.read().await.contains_key("dead"));
        // Live tokens survive the prune pass.
        assert!(st.check(&t1).await);
        st.lock().await;
        assert!(st.tokens.read().await.is_empty());
    }

    #[test]
    fn verify_no_hash_returns_none() {
        let store = Store::memory();
        assert_eq!(password_hash(&store), None);
        store.set_meta(AUTH_PASSWORD_HASH_KEY, "x");
        assert_eq!(password_hash(&store), Some("x".into()));
    }
}

#[cfg(test)]
pub(crate) mod router_tests {
    use super::*;
    use crate::api::build_router;
    use crate::api::tests_common;
    use axum::http::StatusCode;
    use serde_json::Value;
    use tower::ServiceExt;

    async fn call(
        app: axum::Router,
        method: &str,
        uri: &str,
        body: Option<Value>,
        token: Option<&str>,
    ) -> (StatusCode, Value) {
        let mut b = axum::http::Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json");
        if let Some(t) = token {
            b = b.header("authorization", format!("Bearer {t}"));
        }
        let raw = body.map(|v| v.to_string()).unwrap_or_default();
        let req = b
            .body(axum::body::Body::from(raw))
            .expect("valid request");
        let res = app.oneshot(req).await.expect("request ok");
        let status = res.status();
        let v: Value = serde_json::from_slice(
            &axum::body::to_bytes(res.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap_or(Value::Null);
        (status, v)
    }

    #[tokio::test]
    async fn writes_401_without_token() {
        let (st, _pw) = tests_common::state_with_password().await;
        let app = build_router(st);
        let cases = [
            ("POST", "/api/apply", json!({ "gpu": "x" })),
            ("POST", "/api/presets", json!({ "name": "n", "settings": {} })),
            ("PUT", "/api/presets/1", json!({ "name": "n" })),
            ("DELETE", "/api/presets/1", Value::Null),
            ("POST", "/api/setup/ring", json!({ "minutes": 10 })),
            (
                "POST",
                "/api/setup/drift",
                json!({ "reapply": true, "delay_s": 30 }),
            ),
            ("DELETE", "/api/drift", Value::Null),
            ("POST", "/api/revert", json!({})),
            ("POST", "/api/baseline", json!({})),
            ("PUT", "/api/watchdog", json!({})),
            ("POST", "/api/auth/lock", json!({})),
        ];
        for (m, u, b) in cases {
            let (status, v) = call(app.clone(), m, u, Some(b), None).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED, "{m} {u}");
            assert_eq!(v["error"], "authentication required", "{m} {u}");
        }
        // No state changed: the ring window is still the default (the gated
        // `POST /api/setup/ring` above never reached its handler).
        let (_, ring) = call(app.clone(), "GET", "/api/setup/ring", None, None).await;
        assert_eq!(ring["minutes"], 10);
    }

    #[tokio::test]
    async fn reads_open_without_token() {
        let (st, _pw) = tests_common::state_with_password().await;
        let app = build_router(st);
        for u in [
            "/api/status",
            "/api/presets",
            "/api/telemetry",
            "/api/drift",
            "/api/events",
            "/api/setup",
            "/api/exporter/metrics",
        ] {
            let (status, _) = call(app.clone(), "GET", u, None, None).await;
            assert_eq!(status, StatusCode::OK, "{u}");
        }
    }

    #[tokio::test]
    async fn unlock_flow() {
        let (st, pw) = tests_common::state_with_password().await;
        let app = build_router(st.clone());

        // Bad password → 401 + auth_unlock_failed event.
        let (status, _) = call(
            app.clone(),
            "POST",
            "/api/auth/unlock",
            Some(json!({ "password": "wrong" })),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let events = st.store.list_events(None, 100).expect("events");
        assert!(
            events.iter().any(|e| e.kind == "auth_unlock_failed"),
            "expected an auth_unlock_failed event"
        );

        // Correct password → token that unlocks the writes.
        let (status, v) = call(
            app.clone(),
            "POST",
            "/api/auth/unlock",
            Some(json!({ "password": pw })),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let token = v["token"].as_str().expect("token").to_string();
        assert_eq!(v["ttl_s"], 1800);
        let (status, _) = call(
            app.clone(),
            "POST",
            "/api/setup/ring",
            Some(json!({ "minutes": 10 })),
            Some(&token),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        // A second unlock yields a second, independently working token.
        let (_, v2) = call(
            app.clone(),
            "POST",
            "/api/auth/unlock",
            Some(json!({ "password": pw })),
            None,
        )
        .await;
        let token2 = v2["token"].as_str().expect("token2").to_string();
        assert_ne!(token, token2);
        let (status, _) = call(
            app.clone(),
            "POST",
            "/api/setup/ring",
            Some(json!({ "minutes": 15 })),
            Some(&token2),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        // Global lock drops every token.
        let (status, v) = call(
            app.clone(),
            "POST",
            "/api/auth/lock",
            Some(json!({})),
            Some(&token),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(v["locked"], true);
        let (status, _) = call(
            app.clone(),
            "POST",
            "/api/setup/ring",
            Some(json!({ "minutes": 10 })),
            Some(&token),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn unlock_no_password_configured() {
        let st = tests_common::test_state().await;
        let app = build_router(st);
        let (status, v) = call(
            app.clone(),
            "POST",
            "/api/auth/unlock",
            Some(json!({ "password": "x" })),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(v["error"], "no password configured");
    }
}
