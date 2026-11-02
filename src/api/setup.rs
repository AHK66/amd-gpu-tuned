use axum::http::StatusCode;
use axum::{extract::State, Json};
use serde_json::Value;

use crate::api::{json_err, ApiError};
use crate::api::AppState;
use crate::setup::{self, SetupCheck};
use crate::store;

/// Shared body builder (GET /api/setup and POST /api/setup/verify do the
/// same live check).
fn body(chk: SetupCheck) -> Json<Value> {
    Json(serde_json::to_value(chk).unwrap())
}

pub async fn get_setup(State(st): State<AppState>) -> Json<Value> {
    let chk = setup::run(&st.control.gpus(), st.live_mask).await;
    body(chk)
}

pub async fn verify_setup(State(st): State<AppState>) -> Json<Value> {
    let chk = setup::run(&st.control.gpus(), st.live_mask).await;
    body(chk)
}

/// `GET /api/setup/ring` — current persisted ring window in minutes.
pub async fn get_ring(State(st): State<AppState>) -> Json<Value> {
    Json(serde_json::json!({
        "minutes": store::ring_minutes_from_meta(&st.store)
    }))
}

/// `POST /api/setup/ring` — persist the window and apply it to the live
/// ring (spec §4.4). Loose `Value` body so wrong-shaped bodies get the
/// `json_err` 400, not axum's extractor rejection.
pub async fn set_ring(
    State(st): State<AppState>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let minutes = body
        .get("minutes")
        .and_then(Value::as_u64)
        .and_then(|n| u32::try_from(n).ok())
        .filter(|&n| (1..=60).contains(&n))
        .ok_or_else(|| {
            json_err(
                StatusCode::BAD_REQUEST,
                "minutes must be an integer between 1 and 60",
            )
        })?;
    st.store.set_meta("ring_minutes", &minutes.to_string());
    st.telemetry.set_ring_seconds((minutes * 60) as u64);
    Ok(Json(serde_json::json!({ "minutes": minutes })))
}

/// `GET /api/setup/drift` — current drift-recovery options (spec §5.6).
pub async fn get_drift_setup(State(st): State<AppState>) -> Json<Value> {
    Json(serde_json::json!({
        "reapply": store::drift_reapply_from_meta(&st.store),
        "delay_s": store::drift_delay_s_from_meta(&st.store),
    }))
}

/// `POST /api/setup/drift` — persist both options (spec §5.6). Loose `Value`
/// body so wrong-shaped bodies get the `json_err` 400, not axum's extractor
/// rejection (the ring pattern). Both fields are required.
pub async fn set_drift_setup(
    State(st): State<AppState>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let reapply = body
        .get("reapply")
        .and_then(Value::as_bool)
        .ok_or_else(|| {
            json_err(StatusCode::BAD_REQUEST, "reapply must be a boolean")
        })?;
    let delay_s = body
        .get("delay_s")
        .and_then(Value::as_i64)
        .filter(|&n| (5..=600).contains(&n))
        .ok_or_else(|| {
            json_err(
                StatusCode::BAD_REQUEST,
                "delay_s must be an integer between 5 and 600",
            )
        })?;
    st.store.set_meta("drift_reapply", &reapply.to_string());
    st.store.set_meta("drift_reapply_delay_s", &delay_s.to_string());
    Ok(Json(serde_json::json!({ "reapply": reapply, "delay_s": delay_s })))
}

#[cfg(test)]
mod tests {
    use crate::api::build_router;
    use crate::api::tests_common;
    use axum::body::Body;
    use serde_json::{json, Value};
    use tower::ServiceExt;

    async fn get(app: &axum::Router, uri: &str) -> axum::response::Response {
        app.clone()
            .oneshot(
                axum::http::Request::builder()
                    .uri(uri)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    async fn post(
        app: &axum::Router,
        uri: &str,
        body: Value,
    ) -> axum::response::Response {
        app.clone()
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri(uri)
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    async fn body(res: axum::response::Response) -> Value {
        serde_json::from_slice(
            &axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap(),
        )
        .unwrap()
    }

    #[tokio::test]
    async fn ring_get_defaults_to_10() {
        let st = tests_common::test_state().await;
        let app = build_router(st);
        let res = get(&app, "/api/setup/ring").await;
        assert_eq!(res.status(), axum::http::StatusCode::OK);
        assert_eq!(body(res).await, json!({ "minutes": 10 }));
    }

    #[tokio::test]
    async fn ring_post_persists_and_applies_live() {
        let st = tests_common::test_state().await;
        let app = build_router(st.clone());
        let res = post(&app, "/api/setup/ring", json!({ "minutes": 15 })).await;
        assert_eq!(res.status(), axum::http::StatusCode::OK);
        assert_eq!(body(res).await, json!({ "minutes": 15 }));
        assert_eq!(
            st.store.get_meta("ring_minutes").unwrap(),
            Some("15".to_string())
        );
        assert_eq!(st.telemetry.ring_seconds(), 900);
        let res = get(&app, "/api/setup/ring").await;
        assert_eq!(body(res).await, json!({ "minutes": 15 }));
    }

    #[tokio::test]
    async fn ring_post_rejects_out_of_range() {
        let st = tests_common::test_state().await;
        let app = build_router(st.clone());
        for bad in [json!({ "minutes": 0 }), json!({ "minutes": 61 })] {
            let res = post(&app, "/api/setup/ring", bad).await;
            assert_eq!(res.status(), axum::http::StatusCode::BAD_REQUEST);
            let v = body(res).await;
            assert!(v["error"].as_str().is_some());
        }
        assert_eq!(st.store.get_meta("ring_minutes").unwrap(), None);
        assert_eq!(st.telemetry.ring_seconds(), 600);
    }

    #[tokio::test]
    async fn ring_post_rejects_malformed_bodies() {
        let st = tests_common::test_state().await;
        let app = build_router(st.clone());
        for bad in [
            json!({ "minutes": 15.5 }),
            json!({ "minutes": "15" }),
            json!({}),
        ] {
            let res = post(&app, "/api/setup/ring", bad).await;
            assert_eq!(res.status(), axum::http::StatusCode::BAD_REQUEST);
        }
        assert_eq!(st.telemetry.ring_seconds(), 600);
    }

    #[tokio::test]
    async fn drift_setup_get_defaults() {
        let st = tests_common::test_state().await;
        let app = build_router(st);
        let res = get(&app, "/api/setup/drift").await;
        assert_eq!(res.status(), axum::http::StatusCode::OK);
        assert_eq!(body(res).await, json!({ "reapply": true, "delay_s": 30 }));
    }

    #[tokio::test]
    async fn drift_setup_post_persists_both() {
        let st = tests_common::test_state().await;
        let app = build_router(st.clone());
        let res = post(&app, "/api/setup/drift", json!({ "reapply": false, "delay_s": 60 })).await;
        assert_eq!(res.status(), axum::http::StatusCode::OK);
        assert_eq!(body(res).await, json!({ "reapply": false, "delay_s": 60 }));
        assert_eq!(st.store.get_meta("drift_reapply").unwrap(), Some("false".to_string()));
        assert_eq!(st.store.get_meta("drift_reapply_delay_s").unwrap(), Some("60".to_string()));
        let res = get(&app, "/api/setup/drift").await;
        assert_eq!(body(res).await, json!({ "reapply": false, "delay_s": 60 }));
    }

    #[tokio::test]
    async fn drift_setup_post_rejects_invalid() {
        let st = tests_common::test_state().await;
        let app = build_router(st.clone());
        for bad in [
            json!({ "reapply": false, "delay_s": 0 }),
            json!({ "reapply": false, "delay_s": 601 }),
            json!({ "reapply": false, "delay_s": 15.5 }),
            json!({ "reapply": false, "delay_s": "15" }),
            json!({ "reapply": "yes", "delay_s": 30 }),
            json!({ "delay_s": 30 }),
            json!({ "reapply": true }),
            json!({}),
        ] {
            let res = post(&app, "/api/setup/drift", bad).await;
            assert_eq!(res.status(), axum::http::StatusCode::BAD_REQUEST);
            assert!(body(res).await["error"].as_str().is_some());
        }
        // Nothing persisted.
        assert_eq!(st.store.get_meta("drift_reapply").unwrap(), None);
        assert_eq!(st.store.get_meta("drift_reapply_delay_s").unwrap(), None);
    }
}
