use axum::{extract::State, Json};
use serde_json::{json, Value};

use super::AppState;

pub async fn get_status(State(st): State<AppState>) -> Json<Value> {
    let gpus = st.control.gpus();
    let mut arr = Vec::new();
    for g in &gpus {
        let static_info = g.static_info().await.ok();
        let od = g.read_od().await.ok();
        let fan = g.read_fan_curve().await.ok().flatten();
        let cap = g.read_power_cap_w().await;
        arr.push(json!({
            "slot": g.slot,
            "static": static_info,
            "live": { "power_cap_w": cap, "od": od, "fan": fan },
            "drifted": st.drift.is_drifted(&g.slot),
        }));
    }
    let baseline_ts = st
        .store
        .get_meta("stock_baseline_ts")
        .ok()
        .flatten()
        .and_then(|s| s.parse::<i64>().ok());
    let now = crate::store::now_ms();
    Json(json!({
        "ok": true,
        "version": env!("CARGO_PKG_VERSION"),
        "uptime_ms": st.control.uptime_ms(),
        "busy": st.control.is_busy(),
        "privileged": st.privileged,
        "auth_enabled": crate::api::auth::password_hash(&st.store).is_some(),
        "baseline_ts": baseline_ts,
        "baseline_age_ms": baseline_ts.map(|t| now - t),
        "active_state": st.control.active_state(),
        "active_sources": st
            .store
            .get_meta("active_sources")
            .ok()
            .flatten()
            .and_then(|s| serde_json::from_str::<Value>(&s).ok()),
        "gpus": arr,
    }))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use axum::body::Body;
    use crate::api::build_router;
    use crate::api::tests_common;
    use tower::ServiceExt;

    async fn get(app: axum::Router, uri: &str) -> Value {
        let req = axum::http::Request::builder()
            .uri(uri)
            .body(axum::body::Body::empty())
            .unwrap();
        let res = app.oneshot(req).await.unwrap();
        serde_json::from_slice(
            &axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap(),
        )
        .unwrap()
    }
    async fn post(app: axum::Router, uri: &str, body: Value) -> Value {
        let req = axum::http::Request::builder()
            .method("POST")
            .uri(uri)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        let res = app.oneshot(req).await.unwrap();
        serde_json::from_slice(
            &axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap(),
        )
        .unwrap()
    }

    #[tokio::test]
    async fn status_returns_overdrive_and_uptime() {
        let app = build_router(tests_common::test_state().await);
        let req = axum::http::Request::builder()
            .uri("/api/status")
            .body(axum::body::Body::empty())
            .unwrap();
        let res = app.oneshot(req).await.unwrap();
        assert_eq!(res.status(), axum::http::StatusCode::OK);
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["ok"], true);
        assert!(v["uptime_ms"].is_number());
        assert_eq!(v["gpus"][0]["slot"], "1002:75b4-1458:242f-0000:0b:00.0");
        assert!(v["gpus"][0]["static"]["overdrive"].is_boolean());
        // test_state runs as "privileged"; the flag is a plain boolean.
        assert_eq!(v["privileged"], true);
        assert_eq!(v["auth_enabled"], false);
    }

    #[tokio::test]
    async fn status_reports_auth_enabled_when_hash_set() {
        let st = tests_common::test_state().await;
        st.store.set_meta(
            crate::api::auth::AUTH_PASSWORD_HASH_KEY,
            &crate::api::auth::hash_password("pw"),
        );
        let app = build_router(st);
        let v = get(app, "/api/status").await;
        assert_eq!(v["auth_enabled"], true);
    }

    #[tokio::test]
    async fn setup_endpoint_reports_check() {
        let app = build_router(tests_common::test_state().await);
        let req = axum::http::Request::builder()
            .uri("/api/setup")
            .body(axum::body::Body::empty())
            .unwrap();
        let res = app.oneshot(req).await.unwrap();
        assert_eq!(res.status(), axum::http::StatusCode::OK);
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["gpus"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn status_on_mode_reports_drifted_without_adoption() {
        let st = tests_common::test_state_full().await;
        let app = build_router(st.clone());
        let slot = "1002:75b4-1458:242f-0000:0b:00.0";
        let v = post(app.clone(), "/api/apply", json!({ "gpu": slot, "overrides": { "max_mclk_mhz": 1150 } })).await;
        assert_eq!(v["ok"], true);
        // Out-of-band edit to 1310 (staged grammar, like an external tool).
        let od_rel = "card0/device/pp_od_clk_voltage";
        st.control.gpus()[0].root.write(od_rel, "m 1 1310").await.unwrap();
        st.control.gpus()[0].root.write(od_rel, "c").await.unwrap();
        // The guard detects it (re-apply ON is the default).
        st.drift.tick().await;
        let v = get(app.clone(), "/api/status").await;
        assert_eq!(v["gpus"][0]["drifted"], true);
        // No adoption: the source label and intended state are unchanged.
        assert_eq!(v["active_sources"][slot], "adhoc");
        assert_eq!(v["active_state"]["settings"][slot]["max_mclk_mhz"], 1150);
    }

    #[tokio::test]
    async fn status_off_mode_adopts_as_today() {
        let st = tests_common::test_state_full().await;
        st.store.set_meta("drift_reapply", "false");
        let app = build_router(st.clone());
        let slot = "1002:75b4-1458:242f-0000:0b:00.0";
        let v = post(app.clone(), "/api/apply", json!({ "gpu": slot, "overrides": { "max_mclk_mhz": 1150 } })).await;
        assert_eq!(v["ok"], true);
        let od_rel = "card0/device/pp_od_clk_voltage";
        st.control.gpus()[0].root.write(od_rel, "m 1 1310").await.unwrap();
        st.control.gpus()[0].root.write(od_rel, "c").await.unwrap();
        st.drift.tick().await;
        let v = get(app.clone(), "/api/status").await;
        assert_eq!(v["gpus"][0]["drifted"], false);
        assert_eq!(v["active_sources"][slot], "external");
        assert_eq!(v["active_state"]["settings"][slot]["max_mclk_mhz"], 1310);
    }

    #[tokio::test]
    async fn status_no_external_when_matching() {
        let st = tests_common::test_state_full().await;
        let app = build_router(st.clone());
        let slot = "1002:75b4-1458:242f-0000:0b:00.0";
        let v = post(app.clone(), "/api/apply", json!({ "gpu": slot, "overrides": { "max_mclk_mhz": 1150 } })).await;
        assert_eq!(v["ok"], true);
        // No out-of-band change: live OD (1150) matches active_state (1150).
        let v = get(app.clone(), "/api/status").await;
        assert_ne!(v["active_sources"][slot], "external");
    }

    #[tokio::test]
    async fn status_apply_clears_external() {
        // An apply relabels the slot; the guard must not re-flag on the
        // following status call (live now matches the new intended state).
        let st = tests_common::test_state_full().await;
        st.store.set_meta("drift_reapply", "false");
        let app = build_router(st.clone());
        let slot = "1002:75b4-1458:242f-0000:0b:00.0";
        let v = post(app.clone(), "/api/apply", json!({ "gpu": slot, "overrides": { "max_mclk_mhz": 1150 } })).await;
        assert_eq!(v["ok"], true);
        let od_rel = "card0/device/pp_od_clk_voltage";
        st.control.gpus()[0].root.write(od_rel, "m 1 1310").await.unwrap();
        st.control.gpus()[0].root.write(od_rel, "c").await.unwrap();
        st.drift.tick().await;
        let v = get(app.clone(), "/api/status").await;
        assert_eq!(v["active_sources"][slot], "external");
        // A new apply writes 1200 to the hardware and relabels the slot.
        let v = post(app.clone(), "/api/apply", json!({ "gpu": slot, "overrides": { "max_mclk_mhz": 1200 } })).await;
        assert_eq!(v["ok"], true);
        let v = get(app.clone(), "/api/status").await;
        assert_eq!(v["active_sources"][slot], "adhoc");
        assert_eq!(v["gpus"][0]["drifted"], false);
    }
}
