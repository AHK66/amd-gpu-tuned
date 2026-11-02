use axum::{extract::State, http::StatusCode, Json};
use serde_json::{json, Value};

use crate::api::{json_err, ApiError, AppState};
use crate::store;

/// `GET /api/drift` — drift-recovery mode, the ledger (newest first, ≤ 50),
/// and the pending restores (spec §6.5).
pub async fn get_drift(State(st): State<AppState>) -> Json<Value> {
    Json(serde_json::json!({
        "reapply": store::drift_reapply_from_meta(&st.store),
        "delay_s": store::drift_delay_s_from_meta(&st.store),
        "entries": st.store.list_drift_entries().unwrap_or_default(),
        "pending": st.drift.pending_map(),
    }))
}

/// `DELETE /api/drift` — erase the drift history (clear-drift-history spec).
/// Clears pending restore timers first (they reference the rows), then the
/// ledger. Responds with the number of rows removed.
pub async fn clear_drift(State(st): State<AppState>) -> Result<Json<Value>, ApiError> {
    st.drift.clear_pending();
    let n = st
        .store
        .clear_drift_ledger()
        .map_err(|e| json_err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    st.store.append_event("drift_history_cleared", &json!({ "cleared": n }));
    Ok(Json(json!({ "cleared": n })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::build_router;
    use crate::api::tests_common;
    use axum::body::Body;
    use serde_json::{json, Value};
    use tower::ServiceExt;

    async fn get(app: &axum::Router, uri: &str) -> (axum::http::StatusCode, Value) {
        let res = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .uri(uri)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = res.status();
        let v = serde_json::from_slice(
            &axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap(),
        )
        .unwrap();
        (status, v)
    }

    async fn post(app: &axum::Router, uri: &str, body: Value) -> Value {
        let res = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .method(axum::http::Method::POST)
                    .uri(uri)
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        serde_json::from_slice(
            &axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap(),
        )
        .unwrap()
    }

    async fn delete(app: &axum::Router) -> (axum::http::StatusCode, Value) {
        let res = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .method(axum::http::Method::DELETE)
                    .uri("/api/drift")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = res.status();
        let v = serde_json::from_slice(
            &axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap(),
        )
        .unwrap();
        (status, v)
    }

    #[tokio::test]
    async fn drift_get_empty() {
        let st = tests_common::test_state().await;
        let app = build_router(st);
        let (status, v) = get(&app, "/api/drift").await;
        assert_eq!(status, axum::http::StatusCode::OK);
        assert_eq!(
            v,
            json!({ "reapply": true, "delay_s": 30, "entries": [], "pending": {} })
        );
    }

    #[tokio::test]
    async fn drift_get_populated() {
        let st = tests_common::test_state_full().await;
        let app = build_router(st.clone());
        let _ = st.store.append_drift_entry(
            "1002:75b4-1458:242f-0000:0b:00.0",
            &json!({ "power_cap_w": 300.0 }),
            &Some(json!({ "power_cap_w": 235.0 })),
            "adhoc",
        );
        st.store.set_meta("drift_reapply_delay_s", "45");
        let (status, v) = get(&app, "/api/drift").await;
        assert_eq!(status, axum::http::StatusCode::OK);
        assert_eq!(v["reapply"], true);
        assert_eq!(v["delay_s"], 45);
        assert_eq!(v["entries"].as_array().unwrap().len(), 1);
        assert_eq!(v["entries"][0]["previous_source"], "adhoc");
        assert_eq!(v["entries"][0]["outcome"], "pending");
        assert_eq!(v["pending"], json!({}));
    }

    #[tokio::test]
    async fn drift_delete_populated() {
        let st = tests_common::test_state_full().await;
        let app = build_router(st.clone());
        let _ = st.store.append_drift_entry("s1", &json!({ "power_cap_w": 300.0 }), &None, "adhoc");
        let _ = st.store.append_drift_entry("s1", &json!({ "power_cap_w": 290.0 }), &None, "adhoc");
        let (status, v) = delete(&app).await;
        assert_eq!(status, axum::http::StatusCode::OK);
        assert_eq!(v, json!({ "cleared": 2 }));
        let (status, v) = get(&app, "/api/drift").await;
        assert_eq!(status, axum::http::StatusCode::OK);
        assert_eq!(v["entries"], json!([]));
        // A clear event with the count is recorded in the general log.
        let evs = st.store.list_events(None, 100).unwrap();
        let ev = evs.iter().find(|e| e.kind == "drift_history_cleared").unwrap();
        assert_eq!(ev.detail, json!({ "cleared": 2 }));
    }

    #[tokio::test]
    async fn drift_delete_empty() {
        let st = tests_common::test_state().await;
        let app = build_router(st);
        let (status, v) = delete(&app).await;
        assert_eq!(status, axum::http::StatusCode::OK);
        assert_eq!(v, json!({ "cleared": 0 }));
    }

    #[tokio::test]
    async fn drift_delete_clears_pending() {
        let st = tests_common::test_state_full().await;
        let app = build_router(st.clone());
        // Open a pending episode: apply, drift out-of-band, tick (the
        // status.rs `drifted` test pattern).
        let slot = "1002:75b4-1458:242f-0000:0b:00.0";
        let v = post(&app, "/api/apply", json!({ "gpu": slot, "overrides": { "max_mclk_mhz": 1150 } })).await;
        assert_eq!(v["ok"], true);
        let od_rel = "card0/device/pp_od_clk_voltage";
        st.control.gpus()[0].root.write(od_rel, "m 1 1310").await.unwrap();
        st.control.gpus()[0].root.write(od_rel, "c").await.unwrap();
        st.drift.tick().await;
        let (_, v) = get(&app, "/api/drift").await;
        assert!(v["pending"].as_object().unwrap().get(slot).is_some());
        // Clear wipes the rows AND the pending timer.
        let (status, v) = delete(&app).await;
        assert_eq!(status, axum::http::StatusCode::OK);
        assert_eq!(v, json!({ "cleared": 1 }));
        let (_, v) = get(&app, "/api/drift").await;
        assert_eq!(v["entries"], json!([]));
        assert_eq!(v["pending"], json!({}));
    }
}
