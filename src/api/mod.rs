pub mod auth;
pub mod apply;
pub mod drift;
pub mod events;
pub mod exporter;
pub mod presets;
pub mod setup;
pub mod status;
pub mod telemetry;
#[cfg(test)]
pub mod tests_common;
pub mod watchdog;

use std::sync::Arc;

use axum::http::StatusCode;
use axum::Json;

use crate::config::Config;
use crate::control::Control;
use crate::store::Store;

pub type ApiError = (StatusCode, Json<serde_json::Value>);
pub fn json_err(code: StatusCode, msg: &str) -> ApiError {
    (code, Json(serde_json::json!({ "error": msg })))
}

#[derive(Clone)]
pub struct AppState {
    #[allow(dead_code)]
    pub cfg: Arc<Config>,
    pub store: Arc<Store>,
    pub auth: Arc<crate::api::auth::AuthState>,
    pub control: Arc<Control>,
    pub drift: Arc<crate::drift::DriftGuard>,
    /// Overdrive feature mask, read once at bootstrap (tests inject a constant
    /// — the setup endpoint must not read live kernel state in tests).
    pub live_mask: Option<u32>,
    pub telemetry: Arc<crate::telemetry::Telemetry>,
    pub watchdog: Arc<crate::watchdog::Watchdog>,
    /// Whether the process can write the amdgpu sysfs tuning nodes — true only
    /// when running as root (the `sudo` deployment model). False means every
    /// apply/revert/watchdog write is blocked by the OS.
    pub privileged: bool,
}

/// `GET /api` — the service index (moved from `GET /` when the SPA took the
/// root; spec §3.3).
pub(crate) async fn api_index() -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({
        "service": "amd-gpu-tuned",
        "endpoints": [
            "/api/status", "/api/setup", "/api/setup/verify",
            "/api/setup/ring",
            "/api/drift", "/api/setup/drift",
            "/api/telemetry", "/api/telemetry/stream",
            "/api/presets", "/api/apply", "/api/revert", "/api/baseline",
            "/api/events",
            "/api/watchdog", "/api/exporter/metrics",
            "/api/auth/unlock", "/api/auth/lock",
        ],
    }))
}

pub fn build_router(state: AppState) -> axum::Router {
    // Write routes only — the gate layer covers exactly this subrouter
    // (spec §5.4). GET halves of mixed paths stay on the open router below;
    // `merge` combines the disjoint method sets per path.
    let write = axum::Router::new()
        .route("/api/setup/ring", axum::routing::post(setup::set_ring))
        .route(
            "/api/setup/drift",
            axum::routing::post(setup::set_drift_setup),
        )
        .route("/api/drift", axum::routing::delete(drift::clear_drift))
        .route("/api/presets", axum::routing::post(presets::create))
        .route("/api/presets/import", axum::routing::post(presets::import))
        .route(
            "/api/presets/{id}",
            axum::routing::put(presets::update).delete(presets::delete),
        )
        .route("/api/apply", axum::routing::post(apply::apply))
        .route("/api/revert", axum::routing::post(apply::revert))
        .route("/api/baseline", axum::routing::post(apply::baseline))
        .route("/api/watchdog", axum::routing::put(watchdog::put))
        .route("/api/auth/lock", axum::routing::post(auth::lock))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth::gate,
        ));

    axum::Router::new()
        .route("/", axum::routing::get(crate::web::index))
        .route("/api", axum::routing::get(api_index))
        .route("/api/status", axum::routing::get(status::get_status))
        .route("/api/setup", axum::routing::get(setup::get_setup))
        .route(
            "/api/setup/verify",
            axum::routing::post(setup::verify_setup),
        )
        .route("/api/setup/ring", axum::routing::get(setup::get_ring))
        .route("/api/drift", axum::routing::get(drift::get_drift))
        .route(
            "/api/setup/drift",
            axum::routing::get(setup::get_drift_setup),
        )
        .route("/api/presets", axum::routing::get(presets::list))
        .route("/api/presets/{id}", axum::routing::get(presets::get))
        .route(
            "/api/presets/{id}/export",
            axum::routing::get(presets::export),
        )
        .route(
            "/api/telemetry",
            axum::routing::get(telemetry::get_telemetry),
        )
        .route(
            "/api/telemetry/stream",
            axum::routing::get(telemetry::stream),
        )
        .route(
            "/api/exporter/metrics",
            axum::routing::get(exporter::metrics),
        )
        .route("/api/events", axum::routing::get(events::list_events))
        .route("/api/watchdog", axum::routing::get(watchdog::get))
        .route("/api/auth/unlock", axum::routing::post(auth::unlock))
        .merge(write)
        .route(
            "/api/{*path}",
            axum::routing::get(|| async { StatusCode::NOT_FOUND }),
        )
        .route("/{*path}", axum::routing::get(crate::web::asset))
        .with_state(state)
}
