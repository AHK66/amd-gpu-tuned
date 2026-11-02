use std::collections::BTreeMap;

use axum::{extract::State, http::StatusCode, Json};
use serde::Deserialize;
use serde_json::{json, Value};

use super::{json_err, ApiError, AppState};
use crate::control::ControlError;
use crate::types::PresetSettings;

#[derive(Deserialize)]
pub(crate) struct ApplyBody {
    preset_id: Option<i64>,
    gpu: Option<String>,
    overrides: Option<PresetSettings>,
}

#[derive(Deserialize)]
pub(crate) struct RevertBody {
    gpu: Option<String>,
}

fn merge_preset_settings(base: PresetSettings, over: PresetSettings) -> PresetSettings {
    PresetSettings {
        power_cap_w: over.power_cap_w.or(base.power_cap_w),
        sclk_offset_mhz: over.sclk_offset_mhz.or(base.sclk_offset_mhz),
        max_mclk_mhz: over.max_mclk_mhz.or(base.max_mclk_mhz),
        voltage_offset_mv: over.voltage_offset_mv.or(base.voltage_offset_mv),
    }
}

fn ctrl_err(e: ControlError) -> ApiError {
    match e {
        ControlError::Busy => json_err(StatusCode::CONFLICT, &e.to_string()),
        ControlError::Other(e) => json_err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

fn rep_body_json(rep: &crate::control::ApplyReport) -> Value {
    json!({ "ts": rep.ts, "ok": rep.ok, "per_gpu": rep.per_gpu })
}

pub async fn apply(
    State(st): State<AppState>,
    Json(body): Json<ApplyBody>,
) -> Result<Json<Value>, ApiError> {
    // `gpu` is required (spec §6.1): an explicit 400, not the extractor's
    // rejection code for a missing field.
    let Some(gpu) = body.gpu.clone() else {
        return Err(json_err(StatusCode::BAD_REQUEST, "gpu is required"));
    };
    if !st.control.gpus().iter().any(|g| g.slot == gpu) {
        return Err(json_err(
            StatusCode::BAD_REQUEST,
            &format!("unknown gpu: {gpu}"),
        ));
    }
    let (merged, source) = match (body.preset_id, body.overrides) {
        (Some(id), Some(over)) => {
            let p = st
                .store
                .get_preset(id)
                .map_err(|e| json_err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?
                .ok_or_else(|| json_err(StatusCode::NOT_FOUND, "preset not found"))?;
            (merge_preset_settings(p.settings, over), format!("preset:{id}"))
        }
        (Some(id), None) => {
            let p = st
                .store
                .get_preset(id)
                .map_err(|e| json_err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?
                .ok_or_else(|| json_err(StatusCode::NOT_FOUND, "preset not found"))?;
            (p.settings, format!("preset:{id}"))
        }
        (None, Some(over)) => (over, "adhoc".into()),
        (None, None) => {
            return Err(json_err(
                StatusCode::BAD_REQUEST,
                "provide preset_id and/or overrides",
            ))
        }
    };
    // Exactly one card: the map carries only the target slot, so the other
    // card's entry in `apply_settings_locked` is absent and untouched.
    let settings = BTreeMap::from([(gpu.clone(), merged)]);
    match st.control.apply_settings(&settings, &source).await {
        Ok(rep) => {
            st.store.set_active_source(&gpu, &source);
            // The report covers all GPUs (empty vecs for absent slots); the
            // response carries only the target card (spec §6.1).
            let per_gpu = serde_json::Map::from_iter([(
                gpu.clone(),
                rep.per_gpu
                    .get(&gpu)
                    .and_then(|v| serde_json::to_value(v).ok())
                    .unwrap_or(Value::Array(vec![])),
            )]);
            Ok(Json(json!({ "ts": rep.ts, "ok": rep.ok, "per_gpu": per_gpu })))
        }
        Err(e) => Err(ctrl_err(e)),
    }
}

pub async fn revert(
    State(st): State<AppState>,
    // `Option<Json<..>>` deserializes an empty body as `None`, so a bodyless
    // call gets an explicit 400 (plan Global Constraints) instead of the
    // extractor's 415/422 rejection.
    body: Option<Json<RevertBody>>,
) -> Result<Json<Value>, ApiError> {
    let body = body
        .ok_or_else(|| json_err(StatusCode::BAD_REQUEST, "revert requires a JSON body"))?;
    match body.gpu.clone() {
        Some(slot) if !st.control.gpus().iter().any(|g| g.slot == slot) => {
            Err(json_err(StatusCode::BAD_REQUEST, &format!("unknown gpu: {slot}")))
        }
        Some(slot) => match st.control.revert_for_slot(&slot).await {
            Ok(rep) => {
                st.store.set_active_source(&slot, "stock");
                Ok(Json(rep_body_json(&rep)))
            }
            Err(e) => Err(ctrl_err(e)),
        },
        None => match st.control.revert().await {
            Ok(rep) => {
                for g in st.control.gpus() {
                    st.store.set_active_source(&g.slot, "stock");
                }
                Ok(Json(rep_body_json(&rep)))
            }
            Err(e) => Err(ctrl_err(e)),
        },
    }
}

pub async fn baseline(State(st): State<AppState>) -> Result<Json<serde_json::Value>, ApiError> {
    if st.control.is_busy() {
        return Err(json_err(
            StatusCode::CONFLICT,
            "an apply is in progress",
        ));
    }
    match st.control.capture_baseline().await {
        Ok(v) => Ok(Json(v)),
        Err(e) => Err(ctrl_err(e)),
    }
}

#[cfg(test)]
mod tests {
    use crate::api::tests_common;
    use axum::body::Body;
    use serde_json::{json, Value};
    use tower::ServiceExt;

    const SLOT: &str = "1002:75b4-1458:242f-0000:0b:00.0";

    async fn post(app: axum::Router, uri: &str, body: Value) -> axum::response::Response {
        let req = axum::http::Request::builder()
            .method("POST")
            .uri(uri)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        app.oneshot(req).await.unwrap()
    }
    async fn body(res: axum::response::Response) -> Value {
        serde_json::from_slice(
            &axum::body::to_bytes(res.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap()
    }
    async fn del(app: axum::Router, uri: &str) -> axum::response::Response {
        let req = axum::http::Request::builder()
            .method("DELETE")
            .uri(uri)
            .body(Body::empty())
            .unwrap();
        app.oneshot(req).await.unwrap()
    }

    #[tokio::test]
    async fn preset_delete_204_then_404() {
        let st = tests_common::test_state_full().await;
        let app = crate::api::build_router(st);
        let res = post(app.clone(), "/api/presets", json!({ "name": "del-me" })).await;
        let id = body(res).await["id"].as_i64().unwrap();
        let res = del(app.clone(), &format!("/api/presets/{id}")).await;
        assert_eq!(res.status(), axum::http::StatusCode::NO_CONTENT);
        let res = del(app.clone(), &format!("/api/presets/{id}")).await;
        assert_eq!(res.status(), axum::http::StatusCode::NOT_FOUND);
        let res = del(app.clone(), "/api/presets/999999").await;
        assert_eq!(res.status(), axum::http::StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn apply_preset_reports_per_knob() {
        let st = tests_common::test_state_full().await;
        let app = crate::api::build_router(st);
        let res = post(
            app.clone(),
            "/api/presets",
            json!({
                "name": "t",
                "settings": {"power_cap_w": 235.0, "sclk_offset_mhz": 100, "voltage_offset_mv": -50}
            }),
        )
        .await;
        let id = body(res).await["id"].as_i64().unwrap();
        let res = post(
            app.clone(),
            "/api/apply",
            json!({"preset_id": id, "gpu": SLOT}),
        )
        .await;
        assert_eq!(res.status(), axum::http::StatusCode::OK);
        let v = body(res).await;
        assert_eq!(v["ok"], true);
        let outs = v["per_gpu"][SLOT].as_array().unwrap();
        let got: Vec<(&str, &str)> = outs
            .iter()
            .map(|o| (o["field"].as_str().unwrap(), o["status"].as_str().unwrap()))
            .collect();
        assert_eq!(
            got,
            vec![
                ("power_cap_w", "verified"),
                ("sclk_offset_mhz", "verified"),
                ("voltage_offset_mv", "verified"),
            ]
        );
    }

    #[tokio::test]
    async fn apply_override_wins() {
        let st = tests_common::test_state_full().await;
        let app = crate::api::build_router(st.clone());
        // Create a preset with power_cap_w=235.
        let res = post(
            app.clone(),
            "/api/presets",
            json!({
                "name": "merge-test",
                "settings": {"power_cap_w": 235.0, "sclk_offset_mhz": 100}
            }),
        )
        .await;
        let id = body(res).await["id"].as_i64().unwrap();
        // Apply with an override that changes power_cap_w but not sclk.
        let res = post(
            app.clone(),
            "/api/apply",
            json!({
                "preset_id": id,
                "gpu": SLOT,
                "overrides": {"power_cap_w": 250.0}
            }),
        )
        .await;
        assert_eq!(res.status(), axum::http::StatusCode::OK);
        let v = body(res).await;
        assert_eq!(v["ok"], true);
        // The power cap should be 250 (override wins), sclk should be 100 (preset).
        let outs = v["per_gpu"][SLOT].as_array().unwrap();
        let cap = outs.iter().find(|o| o["field"] == "power_cap_w").unwrap();
        assert_eq!(cap["status"], "verified");
        let sclk = outs.iter().find(|o| o["field"] == "sclk_offset_mhz").unwrap();
        assert_eq!(sclk["status"], "verified");
        // Assert the applied VALUE: read back the power cap from the fake GPU.
        // A merge that wrongly kept the preset's 235 W would fail here.
        let applied = st.control.gpus()[0].read_power_cap_w().await.unwrap();
        assert!(
            (applied - 250.0).abs() <= 1.0,
            "expected ~250 W (override wins), got {applied} W"
        );
    }

    #[tokio::test]
    async fn apply_unknown_gpu_400() {
        let st = tests_common::test_state_full().await;
        let app = crate::api::build_router(st);
        let res = post(
            app,
            "/api/apply",
            json!({
                "gpu": "9999:9999-9999:9999-0000:00:00.0",
                "overrides": {"power_cap_w": 250.0}
            }),
        )
        .await;
        assert_eq!(res.status(), axum::http::StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn apply_requires_gpu_400() {
        let st = tests_common::test_state_full().await;
        let app = crate::api::build_router(st);
        // Missing `gpu` -> explicit 400 (spec §6.1), not an extractor rejection.
        let res = post(
            app,
            "/api/apply",
            json!({"overrides": {"power_cap_w": 250.0}}),
        )
        .await;
        assert_eq!(res.status(), axum::http::StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn apply_empty_preset() {
        let st = tests_common::test_state_full().await;
        let app = crate::api::build_router(st.clone());
        // Create a preset with empty settings (no knobs).
        let res = post(
            app.clone(),
            "/api/presets",
            json!({"name": "empty-preset", "settings": {}}),
        )
        .await;
        let id = body(res).await["id"].as_i64().unwrap();
        let res = post(
            app.clone(),
            "/api/apply",
            json!({"preset_id": id, "gpu": SLOT}),
        )
        .await;
        assert_eq!(res.status(), axum::http::StatusCode::OK);
        let v = body(res).await;
        // The target GPU's per_gpu outcome array is empty (no knobs).
        let outs = v["per_gpu"][SLOT].as_array().unwrap();
        assert!(outs.is_empty());
    }

    #[tokio::test]
    async fn apply_with_overrides_only() {
        let st = tests_common::test_state_full().await;
        let app = crate::api::build_router(st);
        let res = post(
            app,
            "/api/apply",
            json!({"gpu": SLOT, "overrides": {"max_mclk_mhz": 1100}}),
        )
        .await;
        assert_eq!(res.status(), axum::http::StatusCode::OK);
        let v = body(res).await;
        assert_eq!(v["ok"], true);
    }

    #[tokio::test]
    async fn apply_neither_preset_nor_overrides_400() {
        let st = tests_common::test_state_full().await;
        let app = crate::api::build_router(st);
        let res = post(app, "/api/apply", json!({"gpu": SLOT})).await;
        assert_eq!(res.status(), axum::http::StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn apply_busy_409() {
        let st = tests_common::test_state_full().await;
        // Clone the Arc first: the guard borrows the control, and
        // build_router takes `st` by value.
        let control = st.control.clone();
        let _guard = control.busy.try_lock().unwrap();
        let app = crate::api::build_router(st);
        let res = post(
            app,
            "/api/apply",
            json!({"gpu": SLOT, "overrides": {"max_mclk_mhz": 1100}}),
        )
        .await;
        assert_eq!(res.status(), axum::http::StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn baseline_then_revert() {
        // test_state_full captures the STOCK baseline at construction —
        // do NOT re-capture after applying, or the baseline records the
        // applied state and the revert becomes a no-op.
        let st = tests_common::test_state_full().await;
        let control = st.control.clone();
        let app = crate::api::build_router(st);
        // apply something
        let res = post(
            app.clone(),
            "/api/apply",
            json!({"gpu": SLOT, "overrides": {"sclk_offset_mhz": 200}}),
        )
        .await;
        assert_eq!(body(res).await["ok"], true);
        let t = control.gpus()[0].read_od().await.unwrap();
        assert_eq!(t.sclk_offset_mhz, 200);
        // revert goes back to the stock baseline
        let res = post(app, "/api/revert", json!({})).await;
        assert_eq!(res.status(), axum::http::StatusCode::OK);
        assert_eq!(body(res).await["ok"], true);
        let t = control.gpus()[0].read_od().await.unwrap();
        assert_eq!(
            t.sclk_offset_mhz, 0,
            "revert must restore stock sclk offset"
        );
    }

    #[tokio::test]
    async fn revert_bodyless_400() {
        let st = tests_common::test_state_full().await;
        let app = crate::api::build_router(st);
        // A bodyless call is a 400 (plan Global Constraints), not the
        // extractor's 415/422 rejection.
        let req = axum::http::Request::builder()
            .method("POST")
            .uri("/api/revert")
            .body(Body::empty())
            .unwrap();
        let res = app.oneshot(req).await.unwrap();
        assert_eq!(res.status(), axum::http::StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn revert_with_gpu_marks_slot_stock() {
        let st = tests_common::test_state_full().await;
        let app = crate::api::build_router(st.clone());
        let res = post(
            app.clone(),
            "/api/apply",
            json!({ "gpu": SLOT, "overrides": { "sclk_offset_mhz": 200 } }),
        )
        .await;
        assert_eq!(res.status(), axum::http::StatusCode::OK);
        let res = post(app.clone(), "/api/revert", json!({ "gpu": SLOT })).await;
        assert_eq!(res.status(), axum::http::StatusCode::OK);
        let src = st.store.get_meta("active_sources").unwrap().unwrap();
        let m: serde_json::Value = serde_json::from_str(&src).unwrap();
        assert_eq!(m[SLOT], "stock");
    }
}
