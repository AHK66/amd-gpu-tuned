use crate::api::AppState;
use crate::config::Config;
use crate::control::Control;
use crate::driver::fake;
use crate::store::Store;
use std::path::PathBuf;
use std::sync::Arc;

pub const OD: &str = "OD_SCLK_OFFSET:\n0Mhz\nOD_MCLK:\n0: 97Mhz\n1: 1258MHz\nOD_VDDGFX_OFFSET:\n0mV\nOD_RANGE:\nSCLK_OFFSET:    -500Mhz       1000Mhz\nMCLK:      97Mhz       1500Mhz\nVDDGFX_OFFSET:    -200mv          0mv";
pub const FAN: &str = "OD_FAN_CURVE:\n0: 25C 30%\n1: 50C 50%\n2: 75C 75%\n3: 100C 100%\nOD_RANGE:\nFAN_CURVE(hotspot temp): 25C 100C\nFAN_CURVE(fan speed): 30% 100%";

pub async fn test_state() -> AppState {
    state_with(&[], true).await
}

/// Full fake tree (OD + fan + caps + DPM gate) with a captured stock
/// baseline — for apply/revert/baseline tests.
pub async fn test_state_full() -> AppState {
    state_with(&[
        ("card0/device/pp_od_clk_voltage", OD),
        ("card0/device/gpu_od/fan_ctrl/fan_curve", FAN),
        ("card0/device/hwmon/hwmon0/power1_cap", "300000000"),
        ("card0/device/hwmon/hwmon0/power1_cap_min", "120000000"),
        ("card0/device/hwmon/hwmon0/power1_cap_max", "300000000"),
        ("card0/device/power_dpm_force_performance_level", "auto"),
    ], true)
    .await
}

/// `test_state_full` plus an edge temp sensor reporting 86 °C against a
/// 90 °C slowdown threshold — for the watchdog `hot` map test.
pub async fn test_state_hot() -> AppState {
    state_with(&[
        ("card0/device/pp_od_clk_voltage", OD),
        ("card0/device/gpu_od/fan_ctrl/fan_curve", FAN),
        ("card0/device/hwmon/hwmon0/power1_cap", "300000000"),
        ("card0/device/hwmon/hwmon0/power1_cap_min", "120000000"),
        ("card0/device/hwmon/hwmon0/power1_cap_max", "300000000"),
        ("card0/device/power_dpm_force_performance_level", "auto"),
        ("card0/device/hwmon/hwmon0/temp1_label", "edge"),
        ("card0/device/hwmon/hwmon0/temp1_crit", "90000"),
        ("card0/device/hwmon/hwmon0/temp1_input", "86000"),
    ], true)
    .await
}

/// `test_state` with an auth password hash stored — the gate is active.
/// Returns the state and the plaintext password.
pub async fn state_with_password() -> (AppState, String) {
    let st = state_with(&[], true).await;
    st.store.set_meta(
        crate::api::auth::AUTH_PASSWORD_HASH_KEY,
        &crate::api::auth::hash_password("correct-horse-battery"),
    );
    (st, "correct-horse-battery".to_string())
}

async fn state_with(files: &[(&str, &str)], privileged: bool) -> AppState {
    let store = Arc::new(Store::memory());
    let g = fake::gpu(files).await;
    // Control::new already returns Arc<Control> — do not wrap again.
    let control = Control::new(vec![g], store.clone());
    let telemetry = crate::telemetry::Telemetry::new(control.clone(), 600, None);
    let watchdog = crate::watchdog::Watchdog::new(
        control.clone(),
        telemetry.clone(),
        store.clone(),
        privileged,
    );
    let drift = crate::drift::DriftGuard::new(control.clone(), store.clone());
    let st = AppState {
        cfg: Arc::new(Config {
            port: Some(8787),
            data: PathBuf::from("data"),
            poll_s: 1.5,
            fake_sysfs: None,
            record_s: 0.0,
            cmd: None,
        }),
        store: store.clone(),
        auth: Arc::new(crate::api::auth::AuthState::new()),
        control,
        live_mask: Some(0xfff7ffff), // constant: tests never read live kernel state
        telemetry,
        watchdog,
        drift,
        privileged,
    };
    // Baseline capture is the normal first-start step; do it here so
    // revert/fan-disable paths have a stock reference.
    st.control.capture_baseline().await.ok();
    st
}
