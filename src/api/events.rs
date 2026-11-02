use axum::{
    extract::{Query, State},
    Json,
};
use serde::Deserialize;

use super::AppState;

#[derive(Deserialize)]
pub struct EventsQuery {
    #[serde(default)]
    since: Option<i64>,
    #[serde(default)]
    limit: Option<i64>,
}

pub async fn list_events(
    State(st): State<AppState>,
    Query(q): Query<EventsQuery>,
) -> Json<serde_json::Value> {
    let limit = q.limit.unwrap_or(200).clamp(1, 1000);
    let evs = st.store.list_events(q.since, limit).unwrap_or_default();
    Json(serde_json::to_value(evs).unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use crate::api::build_router;
    use crate::api::tests_common;
    use axum::body::Body;
    use serde_json::Value;
    use tower::ServiceExt;

    #[tokio::test]
    async fn events_lists_newest_first() {
        let st = tests_common::test_state_full().await; // apply/baseline produced events
        let app = build_router(st);
        let req = axum::http::Request::builder()
            .uri("/api/events?limit=50")
            .body(Body::empty())
            .unwrap();
        let res = app.oneshot(req).await.unwrap();
        assert_eq!(res.status(), axum::http::StatusCode::OK);
        let v: Value = serde_json::from_slice(
            &axum::body::to_bytes(res.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        let arr = v.as_array().unwrap();
        assert!(!arr.is_empty());
        assert!(arr.iter().all(|e| e["kind"].is_string()));
        // newest first
        for pair in arr.windows(2) {
            assert!(pair[0]["ts"].as_i64().unwrap() >= pair[1]["ts"].as_i64().unwrap());
        }
    }
}
