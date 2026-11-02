use axum::{extract::State, http::StatusCode, Json};
use serde::Deserialize;
use serde_json::Value;

use super::{json_err, AppState};

/// `preset_id` is `Option<Option<i64>>`: field absent → `None` (leave
/// unchanged), `null` → `Some(None)` (clear), number → `Some(Some(id))`
/// (set). `Option` would swallow `null` into `None`, so map it explicitly.
fn de_preset_id<'de, D>(d: D) -> Result<Option<Option<i64>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Some(Option::<i64>::deserialize(d)?))
}

// Parsed (so a malformed body still 400s and the route signature is stable)
// but ignored: the watchdog is permanently dormant (2026-10-18), so `put`
// refuses every change. The fields are therefore intentionally unread.
#[derive(Deserialize)]
#[allow(dead_code)]
pub(crate) struct WatchdogBody {
    enabled: Option<bool>,
    #[serde(default, deserialize_with = "de_preset_id")]
    preset_id: Option<Option<i64>>,
}

pub async fn get(State(st): State<AppState>) -> Json<Value> {
    // `status()` includes the `hot` map (latest telemetry + watch state).
    Json(st.watchdog.status().await)
}

pub async fn put(
    State(_st): State<AppState>,
    Json(_body): Json<WatchdogBody>,
) -> Result<Json<Value>, super::ApiError> {
    // Permanently dormant (2026-10-18): the watchdog state can no longer be
    // changed at runtime. The body is still parsed (route signature stable)
    // and then ignored.
    Err(json_err(
        StatusCode::CONFLICT,
        "the watchdog is permanently dormant and cannot be changed",
    ))
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

    async fn put(app: &axum::Router, uri: &str, body: Value) -> axum::response::Response {
        app.clone()
            .oneshot(
                axum::http::Request::builder()
                    .method("PUT")
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
    async fn watchdog_get_default_state() {
        let st = tests_common::test_state_full().await;
        let app = build_router(st);
        let res = get(&app, "/api/watchdog").await;
        assert_eq!(res.status(), axum::http::StatusCode::OK);
        let v = body(res).await;
        assert_eq!(v["enabled"], true);
        // Permanently dormant: never active, even when privileged + enabled.
        assert_eq!(v["active"], false);
        assert_eq!(v["privileged"], true);
        assert!(v["safe_preset"].is_null());
    }

    #[tokio::test]
    async fn watchdog_put_rejected_when_dormant() {
        let st = tests_common::test_state_full().await;
        let app = build_router(st);
        // The watchdog is permanently dormant: any PUT is refused.
        let res = put(&app, "/api/watchdog", json!({"enabled": false})).await;
        assert_eq!(res.status(), axum::http::StatusCode::CONFLICT);
        // State is unchanged: still enabled (stored intent), never active.
        let res = get(&app, "/api/watchdog").await;
        let v = body(res).await;
        assert_eq!(v["enabled"], true);
        assert_eq!(v["active"], false);
    }

    #[tokio::test]
    async fn watchdog_get_reports_hot_map() {
        // `test_state_hot` (Step 5) seeds an edge sensor at 86 °C against a
        // 90 °C slowdown threshold.
        let st = tests_common::test_state_hot().await;
        st.telemetry.poll_once().await;
        st.watchdog.tick().await;
        let app = build_router(st);
        let res = get(&app, "/api/watchdog").await;
        assert_eq!(res.status(), axum::http::StatusCode::OK);
        let v = body(res).await;
        let slot = "1002:75b4-1458:242f-0000:0b:00.0";
        assert_eq!(v["hot"][slot]["temp_c"], 86);
        assert_eq!(v["hot"][slot]["threshold_c"], 90);
        assert!(v["hot"][slot]["hot_for_ms"].is_u64());
    }
}
