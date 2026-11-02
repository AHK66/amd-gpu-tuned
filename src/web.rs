// src/web.rs — serves the embedded SPA (ui/dist) with an SPA fallback.
// Route precedence (api/mod.rs): /api and /api/* win; `/{*path}` catches the
// rest and falls back to index.html so client-side routes + refreshes resolve.
use axum::extract::Path;
use axum::http::{header, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "ui/dist"]
struct Assets;

const PLACEHOLDER: &str =
    "<!doctype html><meta charset=\"utf-8\"><title>amd-gpu-tuned</title>\n\
      <body style=\"font-family:system-ui\"><h2>amd-gpu-tuned UI not built</h2>\n\
     <p>Run <code>cd ui && npm ci && npm run build</code>, then rebuild the daemon.</p>";

/// `GET /` — the SPA entry point.
pub async fn index() -> Response {
    match Assets::get("index.html") {
        Some(html) => Html(String::from_utf8_lossy(&html.data).into_owned()).into_response(),
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
            PLACEHOLDER,
        )
            .into_response(),
    }
}

/// `GET /{*path}` — embedded asset if present, else the SPA fallback.
pub async fn asset(Path(path): Path<String>) -> Response {
    if let Some(content) = Assets::get(&path) {
        // rust-embed v8: MIME type lives on the metadata (behind the
        // `mime-guess` feature); falls back to octet-stream internally.
        let mime = content.metadata.mimetype().to_string();
        return Response::builder()
            .header(header::CONTENT_TYPE, mime)
            .body(axum::body::Body::from(content.data.to_vec()))
            .unwrap();
    }
    index().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower::ServiceExt;

    fn req(uri: &str) -> axum::http::Request<axum::body::Body> {
        axum::http::Request::builder()
            .uri(uri)
            .body(axum::body::Body::empty())
            .unwrap()
    }

    #[tokio::test]
    async fn missing_asset_falls_back_to_index() {
        let a = asset(Path("definitely/not/an/asset.js".into())).await;
        let b = index().await;
        assert_eq!(a.status(), b.status());
    }

    #[tokio::test]
    async fn index_is_html() {
        let res = index().await;
        assert!(matches!(
            res.status(),
            StatusCode::OK | StatusCode::SERVICE_UNAVAILABLE
        ));
        let ct = res.headers().get(header::CONTENT_TYPE).unwrap().to_str().unwrap();
        assert!(ct.starts_with("text/html"), "got {ct}");
    }

    #[tokio::test]
    async fn api_index_lists_endpoints() {
        let st = crate::api::tests_common::test_state().await;
        let app = crate::api::build_router(st);
        let res = app.oneshot(req("/api")).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let eps = v["endpoints"].as_array().unwrap();
        assert!(eps.iter().any(|e| e == "/api/status"));
        assert!(eps.iter().any(|e| e == "/api/watchdog"));
    }

    #[tokio::test]
    async fn root_serves_the_spa_not_the_json_index() {
        let st = crate::api::tests_common::test_state().await;
        let app = crate::api::build_router(st);
        let res = app.oneshot(req("/")).await.unwrap();
        let ct = res.headers().get(header::CONTENT_TYPE).unwrap().to_str().unwrap();
        assert!(ct.starts_with("text/html"), "GET / must serve the SPA, got {ct}");
    }
}
