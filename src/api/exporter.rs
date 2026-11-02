use std::collections::BTreeMap;

use axum::{
    extract::State,
    http::{HeaderName, HeaderValue},
    response::{IntoResponse, Response},
};
use serde_json::Value;

use super::AppState;
use crate::types::PresetSettings;

fn esc(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

pub async fn metrics(State(st): State<AppState>) -> Response {
    let gpus = st.control.gpus();
    let latest = st.telemetry.latest().await;
    let active = st.control.active_state();
    let active_sources: serde_json::Map<String, Value> = st
        .store
        .get_meta("active_sources")
        .ok()
        .flatten()
        .and_then(|s| serde_json::from_str(&s).ok())
        .and_then(|v: Value| v.as_object().cloned())
        .unwrap_or_default();
    let settings: Option<BTreeMap<String, PresetSettings>> = active
        .as_ref()
        .and_then(|a| a.get("settings").cloned())
        .and_then(|v| serde_json::from_value(v).ok());

    let mut out = String::new();
    let mut header = |name: &str, help: &str| {
        out.push_str(&format!("# HELP {name} {help}\n# TYPE {name} gauge\n"));
    };
    header("gpu_tune_up", "Daemon is reporting this GPU");
    header("gpu_tune_power_cap_w", "Applied power cap (W)");
    header("gpu_tune_sclk_offset_mhz", "Applied sclk offset (MHz)");
    header("gpu_tune_voltage_offset_mv", "Applied vddgfx offset (mV)");
    header("gpu_tune_max_mclk_mhz", "Applied max mclk (MHz)");
    header(
        "gpu_tune_preset",
        "Active preset source (preset:<id> | adhoc | stock)",
    );
    header("gpu_tune_verify_status", "1=verified, 0=not verified");

    for g in &gpus {
        let slot = &g.slot;
        out.push_str(&format!("gpu_tune_up{{gpu=\"{}\"}} 1\n", esc(slot)));
        // Verify status is emitted for every GPU (spec §14: a 1/0 gauge) —
        // `0` when there is no `active_state` yet (fresh boot / stock).
        let verified = active
            .as_ref()
            .and_then(|a| a.get("per_gpu"))
            .and_then(|p| p.get(slot))
            .and_then(|o| o.as_array())
            .map(|os| {
                !os.is_empty()
                    && os.iter().any(|o| o["status"].as_str() == Some("verified"))
                    && !os.iter().any(|o| o["status"].as_str() == Some("failed"))
            })
            .unwrap_or(false);
        out.push_str(&format!(
            "gpu_tune_verify_status{{gpu=\"{}\"}} {}\n",
            esc(slot),
            u8::from(verified)
        ));
        let slot_settings = settings
            .as_ref()
            .and_then(|m| m.get(slot))
            .filter(|s| s.knob_count() > 0);
        if let Some(s) = slot_settings {
            if let Some(w) = s.power_cap_w {
                out.push_str(&format!(
                    "gpu_tune_power_cap_w{{gpu=\"{}\"}} {}\n",
                    esc(slot),
                    w
                ));
            }
            if let Some(v) = s.sclk_offset_mhz {
                out.push_str(&format!(
                    "gpu_tune_sclk_offset_mhz{{gpu=\"{}\"}} {}\n",
                    esc(slot),
                    v
                ));
            }
            if let Some(v) = s.voltage_offset_mv {
                out.push_str(&format!(
                    "gpu_tune_voltage_offset_mv{{gpu=\"{}\"}} {}\n",
                    esc(slot),
                    v
                ));
            }
            if let Some(v) = s.max_mclk_mhz {
                out.push_str(&format!(
                    "gpu_tune_max_mclk_mhz{{gpu=\"{}\"}} {}\n",
                    esc(slot),
                    v
                ));
            }
        }
        // The active-preset series is driven by the per-slot `active_sources`
        // map — in the stock state (no entry) it must still show
        // `name="stock"` so the Grafana panel is never empty.
        let source = active_sources
            .get(slot)
            .and_then(|v| v.as_str())
            .unwrap_or("stock");
        out.push_str(&format!(
            "gpu_tune_preset{{name=\"{}\",gpu=\"{}\"}} 1\n",
            esc(source),
            esc(slot)
        ));
        if let Some(sample) = latest.get(slot) {
            if let Some(bits) = &sample.throttle_bits {
                // Bit names are human-readable ("HW Thermal", …) — sanitize
                // into a valid Prometheus metric name ([a-zA-Z_:][a-zA-Z0-9_:]*).
                for bit in bits {
                    let name: String = bit
                        .chars()
                        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
                        .collect();
                    out.push_str(&format!(
                        "gpu_tune_throttle_{name}{{gpu=\"{}\"}} 1\n",
                        esc(slot)
                    ));
                }
            }
        }
    }

    (
        [(
            HeaderName::from_static("content-type"),
            HeaderValue::from_static("text/plain; version=0.0.4; charset=utf-8"),
        )],
        out,
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::api::build_router;
    use crate::api::tests_common;
    use axum::body::Body;
    use serde_json::{json, Value};
    use tower::ServiceExt;

    async fn post(app: &axum::Router, uri: &str, body: Value) -> axum::response::Response {
        let req = axum::http::Request::builder()
            .method("POST")
            .uri(uri)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        app.clone().oneshot(req).await.unwrap()
    }
    async fn body(res: axum::response::Response) -> Value {
        serde_json::from_slice(
            &axum::body::to_bytes(res.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap()
    }

    #[tokio::test]
    async fn exporter_emits_applied_state() {
        let st = tests_common::test_state_full().await;
        // Apply a preset (power cap + sclk) and name it.
        let settings = BTreeMap::from([(
            "1002:75b4-1458:242f-0000:0b:00.0".to_string(),
            crate::types::PresetSettings {
                power_cap_w: Some(235.0),
                sclk_offset_mhz: Some(100),
                ..Default::default()
            },
        )]);
        st.store
            .set_active_source("1002:75b4-1458:242f-0000:0b:00.0", "preset:7");
        st.control.apply_settings(&settings, "test").await.unwrap();
        let app = build_router(st);
        let req = axum::http::Request::builder()
            .uri("/api/exporter/metrics")
            .body(Body::empty())
            .unwrap();
        let res = app.oneshot(req).await.unwrap();
        assert_eq!(res.status(), axum::http::StatusCode::OK);
        assert!(res.headers()["content-type"]
            .to_str()
            .unwrap()
            .starts_with("text/plain"));
        let body = String::from_utf8(
            axum::body::to_bytes(res.into_body(), usize::MAX)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        let slot = "1002:75b4-1458:242f-0000:0b:00.0";
        assert!(body.contains(&format!("gpu_tune_up{{gpu=\"{slot}\"}} 1")));
        assert!(body.contains(&format!("gpu_tune_power_cap_w{{gpu=\"{slot}\"}} 235")));
        assert!(body.contains(&format!("gpu_tune_sclk_offset_mhz{{gpu=\"{slot}\"}} 100")));
        assert!(body.contains(&format!("gpu_tune_verify_status{{gpu=\"{slot}\"}} 1")));
        assert!(body.contains(&format!(
            "gpu_tune_preset{{name=\"preset:7\",gpu=\"{slot}\"}} 1"
        )));
    }

    #[tokio::test]
    async fn exporter_stock_state_emits_preset_and_verify_zero() {
        // No apply, no `active_sources` meta: the preset series must still
        // appear (name="stock") and verify_status must be 0 — a missing
        // series would leave the Grafana panels empty on a fresh boot.
        let st = tests_common::test_state_full().await;
        let app = build_router(st);
        let req = axum::http::Request::builder()
            .uri("/api/exporter/metrics")
            .body(Body::empty())
            .unwrap();
        let res = app.oneshot(req).await.unwrap();
        let body = String::from_utf8(
            axum::body::to_bytes(res.into_body(), usize::MAX)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        let slot = "1002:75b4-1458:242f-0000:0b:00.0";
        assert!(body.contains(&format!(
            "gpu_tune_preset{{name=\"stock\",gpu=\"{slot}\"}} 1"
        )));
        assert!(body.contains(&format!("gpu_tune_verify_status{{gpu=\"{slot}\"}} 0")));
    }

    #[tokio::test]
    async fn exporter_per_slot_metrics_only_for_configured_slots() {
        let st = tests_common::test_state_full().await;
        // Apply a preset that only configures the one fake GPU's slot.
        let settings = BTreeMap::from([(
            "1002:75b4-1458:242f-0000:0b:00.0".to_string(),
            crate::types::PresetSettings {
                power_cap_w: Some(235.0),
                sclk_offset_mhz: Some(100),
                ..Default::default()
            },
        )]);
        st.control.apply_settings(&settings, "test").await.unwrap();
        let app = build_router(st);
        let req = axum::http::Request::builder()
            .uri("/api/exporter/metrics")
            .body(Body::empty())
            .unwrap();
        let res = app.oneshot(req).await.unwrap();
        let body = String::from_utf8(
            axum::body::to_bytes(res.into_body(), usize::MAX)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        let slot = "1002:75b4-1458:242f-0000:0b:00.0";
        // The configured slot emits its metrics.
        assert!(body.contains(&format!("gpu_tune_power_cap_w{{gpu=\"{slot}\"}} 235")));
        assert!(body.contains(&format!("gpu_tune_sclk_offset_mhz{{gpu=\"{slot}\"}} 100")));
        assert!(body.contains(&format!("gpu_tune_verify_status{{gpu=\"{slot}\"}} 1")));
    }

    #[tokio::test]
    async fn exporter_no_metrics_for_unconfigured_slot() {
        let st = tests_common::test_state_full().await;
        // Apply a preset with an empty map (no slots configured).
        let settings: BTreeMap<String, crate::types::PresetSettings> = BTreeMap::new();
        st.control.apply_settings(&settings, "test").await.unwrap();
        let app = build_router(st);
        let req = axum::http::Request::builder()
            .uri("/api/exporter/metrics")
            .body(Body::empty())
            .unwrap();
        let res = app.oneshot(req).await.unwrap();
        let body = String::from_utf8(
            axum::body::to_bytes(res.into_body(), usize::MAX)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        let slot = "1002:75b4-1458:242f-0000:0b:00.0";
        // No power_cap or sclk metrics for the unconfigured slot.
        assert!(!body.contains(&format!("gpu_tune_power_cap_w{{gpu=\"{slot}\"}}")));
        assert!(!body.contains(&format!("gpu_tune_sclk_offset_mhz{{gpu=\"{slot}\"}}")));
        // But the GPU is still up and the preset series is emitted.
        assert!(body.contains(&format!("gpu_tune_up{{gpu=\"{slot}\"}} 1")));
        assert!(body.contains(&format!(
            "gpu_tune_preset{{name=\"stock\",gpu=\"{slot}\"}} 1"
        )));
    }

    #[tokio::test]
    async fn apply_and_revert_sets_active_sources() {
        // The `active_sources` wiring lives in the handlers (Step 5) — assert
        // it through the real API, not by setting the meta by hand.
        let st = tests_common::test_state_full().await;
        let control = st.control.clone();
        let app = build_router(st);
        let slot = "1002:75b4-1458:242f-0000:0b:00.0";
        let res = post(
            &app,
            "/api/apply",
            json!({"gpu": slot, "overrides": {"power_cap_w": 250.0}}),
        )
        .await;
        assert_eq!(body(res).await["ok"], true);
        let src = control.store.get_meta("active_sources").unwrap().unwrap();
        let m: Value = serde_json::from_str(&src).unwrap();
        assert_eq!(
            m[slot].as_str(),
            Some("adhoc"),
            "overrides-only apply must record the adhoc source"
        );
        let res = post(&app, "/api/revert", json!({})).await;
        assert_eq!(res.status(), axum::http::StatusCode::OK);
        let src = control.store.get_meta("active_sources").unwrap().unwrap();
        let m: Value = serde_json::from_str(&src).unwrap();
        assert_eq!(m[slot].as_str(), Some("stock"), "revert must record stock");
    }
}
