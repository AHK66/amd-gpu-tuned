use axum::{
    extract::State,
    response::sse::{Event, Sse},
    Json,
};
use futures_util::StreamExt;
use serde_json::Value;
use tokio_stream::wrappers::BroadcastStream;

use super::AppState;

pub async fn get_telemetry(State(st): State<AppState>) -> Json<Value> {
    Json(serde_json::json!({
        "latest": st.telemetry.latest().await,
        "ring": st.telemetry.ring().await,
    }))
}

pub async fn stream(
    State(st): State<AppState>,
) -> Sse<impl futures_util::Stream<Item = Result<Event, axum::Error>>> {
    let rx = st.telemetry.subscribe();
    // `StreamExt::filter_map` requires a closure returning a Future —
    // wrap the body in `async move { ... }`.
    let stream = BroadcastStream::new(rx).filter_map(|res| {
        async move {
            match res {
                Ok(sample) => {
                    let data = serde_json::to_string(&sample).ok()?;
                    Some(Ok(Event::default().data(data)))
                }
                Err(_) => None, // lagged or sender closed; skip
            }
        }
    });
    Sse::new(stream).keep_alive(axum::response::sse::KeepAlive::default())
}

#[cfg(test)]
mod tests {
    use crate::api::build_router;
    use crate::api::tests_common;
    use axum::body::Body;
    use futures_util::StreamExt;
    use serde_json::Value;
    use tower::ServiceExt;

    #[tokio::test]
    async fn telemetry_endpoint_shape() {
        let st = tests_common::test_state().await;
        st.telemetry.poll_once().await;
        let app = build_router(st);
        let req = axum::http::Request::builder()
            .uri("/api/telemetry")
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
        assert!(v["latest"]
            .get("1002:75b4-1458:242f-0000:0b:00.0")
            .is_some());
        assert!(v["ring"].is_array());
    }

    #[tokio::test]
    async fn sse_stream_emits_sample() {
        let st = tests_common::test_state().await;
        let st2 = st.clone(); // build_router takes ownership; share the Arc<Telemetry>
        let app = build_router(st);
        let req = axum::http::Request::builder()
            .uri("/api/telemetry/stream")
            .body(Body::empty())
            .unwrap();
        // oneshot resolves once headers are ready; sample after connect.
        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), axum::http::StatusCode::OK);
        assert!(res.headers()["content-type"]
            .to_str()
            .unwrap()
            .contains("text/event-stream"));
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            st2.telemetry.poll_once().await;
        });
        let mut stream = res.into_body().into_data_stream();
        let chunk = tokio::time::timeout(std::time::Duration::from_secs(2), stream.next())
            .await
            .expect("no SSE data in 2s")
            .expect("stream ended")
            .unwrap();
        let s = String::from_utf8_lossy(&chunk);
        assert!(s.contains("data:"));
        assert!(s.contains("1002:75b4-1458:242f-0000:0b:00.0"));
    }
}
