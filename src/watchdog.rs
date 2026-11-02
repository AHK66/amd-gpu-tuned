use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};

use crate::control::Control;
use crate::store::{now_ms, Store};
use crate::telemetry::Telemetry;
use crate::types::{GpuSample, GpuStatic};

/// Production defaults (spec §4). Tests inject smaller values via
/// [`WatchdogConfig`] so a trigger fires in < 1 s (spec §8).
pub const PROXIMITY_C: i32 = 5;
pub const SUSTAIN_MS: i64 = 30_000;
pub const REARM_MARGIN_C: i32 = 10;

/// Bounded waits so a stuck apply cannot hang the watchdog.
const APPLY_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy)]
pub struct WatchdogConfig {
    pub proximity_c: i32,
    pub sustain_ms: i64,
    pub rearm_margin_c: i32,
}

impl WatchdogConfig {
    /// Production constants.
    pub fn production() -> Self {
        Self {
            proximity_c: PROXIMITY_C,
            sustain_ms: SUSTAIN_MS,
            rearm_margin_c: REARM_MARGIN_C,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GpuState {
    Armed,
    Hot,
    Triggered,
}

#[derive(Debug, Clone)]
struct GpuWatch {
    state: GpuState,
    hot_since_ms: i64,
}

pub struct Watchdog {
    control: Arc<Control>,
    telemetry: Arc<Telemetry>,
    store: Arc<Store>,
    cfg: WatchdogConfig,
    enabled: AtomicBool,
    /// Can the daemon write to the card (running as root)? The watchdog is
    /// only *active* when `enabled && privileged` — an armed-but-unprivileged
    /// watchdog would fire a safe-preset apply that the OS rejects, i.e. it
    /// would look like protection while doing nothing.
    privileged: bool,
    safe_preset_id: std::sync::Mutex<Option<i64>>,
    last_trigger: std::sync::Mutex<Option<Value>>,
    watches: std::sync::Mutex<BTreeMap<String, GpuWatch>>,
}

impl Watchdog {
    /// Production constructor (production defaults from
    /// [`WatchdogConfig::production`]).
    pub fn new(
        control: Arc<Control>,
        telemetry: Arc<Telemetry>,
        store: Arc<Store>,
        privileged: bool,
    ) -> Arc<Self> {
        Self::new_with(
            WatchdogConfig::production(),
            control,
            telemetry,
            store,
            privileged,
        )
    }

    pub fn new_with(
        cfg: WatchdogConfig,
        control: Arc<Control>,
        telemetry: Arc<Telemetry>,
        store: Arc<Store>,
        privileged: bool,
    ) -> Arc<Self> {
        let enabled = store
            .get_meta("watchdog_enabled")
            .ok()
            .flatten()
            .map(|s| s != "false")
            .unwrap_or(true);
        let safe_preset_id = store
            .get_meta("watchdog_safe_preset")
            .ok()
            .flatten()
            .and_then(|s| s.parse::<i64>().ok());
        let last_trigger = store
            .get_meta("watchdog_last_trigger")
            .ok()
            .flatten()
            .and_then(|s| serde_json::from_str(&s).ok());
        Arc::new(Self {
            control,
            telemetry,
            store,
            cfg,
            enabled: AtomicBool::new(enabled),
            privileged,
            safe_preset_id: std::sync::Mutex::new(safe_preset_id),
            last_trigger: std::sync::Mutex::new(last_trigger),
            watches: std::sync::Mutex::new(BTreeMap::new()),
        })
    }

    pub fn set_enabled(&self, on: bool) {
        self.enabled.store(on, Ordering::Relaxed);
        self.store.set_meta("watchdog_enabled", &on.to_string());
    }

    pub fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    /// Permanently dormant: the watchdog never activates, regardless of the
    /// `enabled` flag or privilege. The capability stays in the tree but is
    /// inert. `enabled`/`privileged` are reported by `status` for
    /// transparency only.
    pub fn active(&self) -> bool {
        false
    }

    pub fn set_safe_preset(&self, id: Option<i64>) {
        *self.safe_preset_id.lock().unwrap() = id;
        match id {
            Some(i) => self.store.set_meta("watchdog_safe_preset", &i.to_string()),
            None => self.store.set_meta("watchdog_safe_preset", ""),
        }
    }

    pub fn safe_preset_id(&self) -> Option<i64> {
        *self.safe_preset_id.lock().unwrap()
    }

    /// `GET /api/watchdog` body (spec §4): enabled, safe_preset,
    /// last_trigger, and the current `hot` map.
    pub async fn status(&self) -> Value {
        let safe_id = self.safe_preset_id();
        let safe_name = safe_id
            .and_then(|id| self.store.get_preset(id).ok().flatten())
            .map(|p| p.name);
        let last_trigger = self.last_trigger.lock().unwrap().clone();
        json!({
            "enabled": self.enabled(),
            "active": self.active(),
            "privileged": self.privileged,
            "safe_preset": safe_id.map(|id| json!({ "id": id, "name": safe_name })),
            "last_trigger": last_trigger,
            "hot": self.hot_map().await,
        })
    }

    /// Slots whose latest sample is within `proximity_c` of a slowdown
    /// threshold, with the driving sensor's temp/threshold and how long the
    /// GPU has been in the `Hot` state. Used by `status`.
    async fn hot_map(&self) -> Value {
        let latest = self.telemetry.latest().await;
        let gpus = self.control.gpus();
        let now = now_ms();
        let mut out = serde_json::Map::new();
        for g in &gpus {
            let Some(sample) = latest.get(&g.slot) else {
                continue;
            };
            let Ok(static_info) = g.static_info().await else {
                continue;
            };
            let Some((temp, threshold)) =
                Self::driving_hot_sensor(sample, &static_info, self.cfg.proximity_c)
            else {
                continue;
            };
            // Brief lock, never held across an await.
            let hot_for_ms = self
                .watches
                .lock()
                .unwrap()
                .get(&g.slot)
                .filter(|w| w.state == GpuState::Hot)
                .map(|w| (now - w.hot_since_ms).max(0))
                .unwrap_or(0);
            out.insert(
                g.slot.clone(),
                json!({
                    "temp_c": temp,
                    "threshold_c": threshold,
                    "hot_for_ms": hot_for_ms,
                }),
            );
        }
        Value::Object(out)
    }

    fn is_hot(temp: Option<i32>, threshold: Option<i32>, proximity_c: i32) -> bool {
        match (temp, threshold) {
            (Some(t), Some(th)) => t >= th - proximity_c,
            _ => false,
        }
    }

    /// Cool enough to re-arm: at least `rearm_margin_c` below threshold. An
    /// absent sensor (or missing threshold) is not a hot factor, so it does
    /// not block re-arm.
    fn is_cool(temp: Option<i32>, threshold: Option<i32>, rearm_margin_c: i32) -> bool {
        match (temp, threshold) {
            (Some(t), Some(th)) => t <= th - rearm_margin_c,
            _ => true,
        }
    }

    /// The sensor currently closest to (or over) its threshold, if any is
    /// hot. Used by `hot_map`/`status`.
    fn driving_hot_sensor(
        sample: &GpuSample,
        static_info: &GpuStatic,
        proximity_c: i32,
    ) -> Option<(i32, i32)> {
        [
            (sample.edge_c, static_info.slowdown_edge_c),
            (sample.hotspot_c, static_info.slowdown_hotspot_c),
            (sample.vram_c, static_info.slowdown_vram_c),
        ]
        .into_iter()
        .filter_map(|(t, th)| match (t, th) {
            (Some(t), Some(th)) if Self::is_hot(Some(t), Some(th), proximity_c) => Some((t, th)),
            _ => None,
        })
        .min_by_key(|(t, th)| th - t)
    }

    /// One pure state-machine step for one GPU (spec §4). Returns `true`
    /// only on the tick that transitions the GPU `Hot -> Triggered` (never
    /// re-triggers while already `Triggered`; re-arm requires all sensors
    /// cool).
    fn evaluate(
        sample: &GpuSample,
        static_info: &GpuStatic,
        now_ms: i64,
        watches: &mut BTreeMap<String, GpuWatch>,
        cfg: &WatchdogConfig,
    ) -> bool {
        let hot = Self::is_hot(sample.edge_c, static_info.slowdown_edge_c, cfg.proximity_c)
            || Self::is_hot(sample.hotspot_c, static_info.slowdown_hotspot_c, cfg.proximity_c)
            || Self::is_hot(sample.vram_c, static_info.slowdown_vram_c, cfg.proximity_c);
        let w = watches.entry(sample.slot.clone()).or_insert(GpuWatch {
            state: GpuState::Armed,
            hot_since_ms: now_ms,
        });
        let mut just_triggered = false;
        match w.state {
            GpuState::Armed if hot => {
                w.state = GpuState::Hot;
                w.hot_since_ms = now_ms;
            }
            GpuState::Armed => {}
            GpuState::Hot => {
                if !hot {
                    w.state = GpuState::Armed;
                } else if now_ms - w.hot_since_ms >= cfg.sustain_ms {
                    w.state = GpuState::Triggered;
                    just_triggered = true;
                }
            }
            GpuState::Triggered => {
                let cool = Self::is_cool(
                    sample.edge_c,
                    static_info.slowdown_edge_c,
                    cfg.rearm_margin_c,
                ) && Self::is_cool(
                    sample.hotspot_c,
                    static_info.slowdown_hotspot_c,
                    cfg.rearm_margin_c,
                ) && Self::is_cool(
                    sample.vram_c,
                    static_info.slowdown_vram_c,
                    cfg.rearm_margin_c,
                );
                if cool {
                    w.state = GpuState::Armed;
                }
            }
        }
        just_triggered
    }

    /// One evaluation pass over all GPUs (the telemetry cadence).
    pub async fn tick(&self) {
        // Gate on `active`, not `enabled`: an enabled-but-unprivileged daemon
        // cannot write the safe preset, so it must not evaluate/trigger.
        if !self.active() {
            return;
        }
        let latest = self.telemetry.latest().await;
        let gpus = self.control.gpus();
        let now = now_ms();
        // Collect samples first: the watch-map lock must not be held
        // across an await.
        let mut evals: Vec<(&GpuSample, GpuStatic)> = Vec::new();
        for g in &gpus {
            let Some(sample) = latest.get(&g.slot) else {
                continue;
            };
            let Ok(static_info) = g.static_info().await else {
                continue;
            };
            evals.push((sample, static_info));
        }
        let mut triggered: Vec<String> = Vec::new();
        {
            let mut watches = self.watches.lock().unwrap();
            for (sample, static_info) in &evals {
                if Self::evaluate(sample, static_info, now, &mut watches, &self.cfg) {
                    triggered.push(sample.slot.clone());
                }
            }
        }
        for slot in triggered {
            let Some((sample, static_info)) = evals.iter().find(|(s, _)| s.slot == slot)
            else {
                continue;
            };
            let temps = json!({
                "edge_c": sample.edge_c,
                "hotspot_c": sample.hotspot_c,
                "vram_c": sample.vram_c,
            });
            let threshold = static_info
                .slowdown_hotspot_c
                .or(static_info.slowdown_edge_c)
                .unwrap_or(0);
            self.trigger(&slot, temps, threshold).await;
        }
    }

    /// Trigger action (spec §4): apply the safe preset (or revert to
    /// stock), append a `watchdog` event.
    async fn trigger(&self, slot: &str, temps: Value, threshold: i32) {
        // 1. Safe preset if designated, else the stock baseline. Blocks on
        //    the `busy` mutex (a manual apply may hold it), bounded so a
        //    stuck apply cannot hang the watchdog; failures are audited.
        let safe_id = self.safe_preset_id();
        let action = match safe_id.and_then(|id| self.store.get_preset(id).ok().flatten()) {
            Some(preset) => {
                let map: std::collections::BTreeMap<String, crate::types::PresetSettings> = self
                    .control
                    .gpus()
                    .iter()
                    .map(|g| (g.slot.clone(), preset.settings.clone()))
                    .collect();
                match tokio::time::timeout(
                    APPLY_TIMEOUT,
                    self.control.apply_settings_blocking(&map, "watchdog"),
                )
                .await
                {
                    Ok(Ok(_)) => {
                        let source = format!("preset:{}", preset.id.unwrap_or(-1));
                        for slot in map.keys() {
                            self.store.set_active_source(slot, &source);
                        }
                        format!("safe_preset:{}", preset.name)
                    }
                    Ok(Err(e)) => {
                        tracing::warn!("watchdog safe-preset apply failed: {e}");
                        "safe_preset_failed".to_string()
                    }
                    Err(_) => {
                        tracing::warn!("watchdog safe-preset apply timed out");
                        "safe_preset_timed_out".to_string()
                    }
                }
            },
            None => self.revert_stock().await,
        };

        // 2. Audit.
        let detail = json!({
            "slot": slot,
            "temps": temps,
            "threshold": threshold,
            "action": action,
            "preset": safe_id,
        });
        self.store.append_event("watchdog", &detail);
        let record = json!({
            "ts_ms": now_ms(),
            "slot": slot,
            "action": action,
            "detail": temps,
        });
        *self.last_trigger.lock().unwrap() = Some(record.clone());
        self.store.set_meta("watchdog_last_trigger", &record.to_string());
    }

    async fn revert_stock(&self) -> String {
        match tokio::time::timeout(APPLY_TIMEOUT, self.control.revert_blocking()).await {
            Ok(Ok(_)) => {
                for g in self.control.gpus() {
                    self.store.set_active_source(&g.slot, "stock");
                }
                "stock_baseline".to_string()
            }
            Ok(Err(e)) => {
                tracing::warn!("watchdog revert failed: {e}");
                "revert_failed".to_string()
            }
            Err(_) => {
                tracing::warn!("watchdog revert timed out");
                "revert_timed_out".to_string()
            }
        }
    }

    /// Spawn the background loop on the telemetry cadence.
    pub fn start(self: &Arc<Self>, interval: Duration) {
        let w = self.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(interval);
            loop {
                tick.tick().await;
                w.tick().await;
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::driver::fake;
    use crate::store::Store;
    use crate::types::{GpuSample, GpuStatic};
    use std::sync::Arc;

    const SLOT: &str = "1002:75b4-1458:242f-0000:0b:00.0";

    /// Edge sensor at 86 °C against a 90 °C slowdown threshold: hot. Also
    /// carries the power-cap files so the safe-preset apply works.
    const HOT_FILES: &[(&str, &str)] = &[
        ("card0/device/hwmon/hwmon0/power1_average", "299000000"),
        ("card0/device/hwmon/hwmon0/power1_cap", "300000000"),
        ("card0/device/hwmon/hwmon0/power1_cap_min", "120000000"),
        ("card0/device/hwmon/hwmon0/power1_cap_max", "300000000"),
        ("card0/device/power_dpm_force_performance_level", "auto"),
        ("card0/device/hwmon/hwmon0/temp1_label", "edge"),
        ("card0/device/hwmon/hwmon0/temp1_crit", "90000"),
        ("card0/device/hwmon/hwmon0/temp1_input", "86000"),
    ];

    /// Fast config: 100 ms sustain — a trigger fires in < 1 s (spec §8
    /// "injectable timing constants").
    fn fast_cfg() -> WatchdogConfig {
        WatchdogConfig {
            proximity_c: 5,
            sustain_ms: 100,
            rearm_margin_c: 10,
        }
    }

    type TestCtx = (Arc<Watchdog>, Arc<Control>, Arc<Store>, Arc<Telemetry>);

    fn make_watchdog() -> TestCtx {
        let store = Arc::new(Store::memory());
        let g = fake::gpu_sync(HOT_FILES);
        let control = Control::new(vec![g], store.clone());
        let telemetry = Telemetry::new(control.clone(), 600, None);
        let wd = Watchdog::new_with(
            fast_cfg(),
            control.clone(),
            telemetry.clone(),
            store.clone(),
            true, // privileged: the fire-on-hot tests exercise the armed path
        );
        (wd, control, store, telemetry)
    }

    fn hot_sample() -> GpuSample {
        GpuSample {
            slot: SLOT.into(),
            ts_ms: now_ms(),
            socket_power_w: 299.0,
            power_cap_w: Some(300.0),
            edge_c: Some(86),
            hotspot_c: None,
            vram_c: None,
            gfx_mhz: None,
            mem_mhz: None,
            gfx_activity_pct: Some(100),
            mem_activity_pct: Some(50),
            vram_used_mb: None,
            vram_total_mb: None,
            dpm_sclk_mhz: None,
            dpm_mclk_mhz: None,
            throttle_bits: None,
            fan_rpm: None,
            fan_pwm_pct: None,
            voltage_gfx_mv: None,
            voltage_soc_mv: None,
            voltage_mem_mv: None,
            ecc_umc_ce: None,
            ecc_umc_ue: None,
            ecc_umc_deferred: None,
            pcie_width: None,
            pcie_speed_gt: None,
        }
    }

    fn cool_sample() -> GpuSample {
        GpuSample {
            edge_c: Some(70),
            ..hot_sample()
        }
    }

    fn static_info() -> GpuStatic {
        GpuStatic {
            slot: SLOT.into(),
            card_n: 0,
            vbios: None,
            slowdown_edge_c: Some(90),
            slowdown_hotspot_c: None,
            slowdown_vram_c: None,
            cap_min_w: Some(120),
            cap_max_w: Some(300),
            cap_default_w: None,
            dpm_sclk_levels: vec![],
            dpm_mclk_levels: vec![],
            overdrive: true,
            sclk_offset_range_mhz: None,
            mclk_range_mhz: None,
            voltage_offset_range_mv: None,
            pmfw_fan: false,
            thermal_target: false,
            power_profile: false,
        }
    }

    #[test]
    fn state_machine_trigger_sustain_and_hysteresis() {
        let mut watches: BTreeMap<String, GpuWatch> = BTreeMap::new();
        let si = static_info();
        let cfg = fast_cfg();

        // Hot tick 1: Armed -> Hot.
        assert!(!Watchdog::evaluate(&hot_sample(), &si, 1_000, &mut watches, &cfg));
        assert_eq!(watches[SLOT].state, GpuState::Hot);
        // Still hot, before the sustain elapses.
        assert!(!Watchdog::evaluate(&hot_sample(), &si, 1_050, &mut watches, &cfg));
        assert_eq!(watches[SLOT].state, GpuState::Hot);
        // Sustain elapsed: triggers exactly once.
        assert!(Watchdog::evaluate(&hot_sample(), &si, 1_100, &mut watches, &cfg));
        assert_eq!(watches[SLOT].state, GpuState::Triggered);
        // While Triggered: no re-trigger.
        assert!(!Watchdog::evaluate(&hot_sample(), &si, 2_000, &mut watches, &cfg));
        // Cool below the re-arm margin: re-arm.
        assert!(!Watchdog::evaluate(&cool_sample(), &si, 3_000, &mut watches, &cfg));
        assert_eq!(watches[SLOT].state, GpuState::Armed);
        // Hot again: the full cycle re-triggers after re-arm.
        assert!(!Watchdog::evaluate(&hot_sample(), &si, 4_000, &mut watches, &cfg));
        assert!(Watchdog::evaluate(&hot_sample(), &si, 4_100, &mut watches, &cfg));
    }

    #[test]
    fn state_machine_brief_spike_does_not_trigger() {
        let mut watches: BTreeMap<String, GpuWatch> = BTreeMap::new();
        let si = static_info();
        let cfg = fast_cfg();
        assert!(!Watchdog::evaluate(&hot_sample(), &si, 1_000, &mut watches, &cfg));
        // Cools before the sustain elapses.
        assert!(!Watchdog::evaluate(&cool_sample(), &si, 1_050, &mut watches, &cfg));
        assert_eq!(watches[SLOT].state, GpuState::Armed);
        // A fresh hot streak must re-accumulate the sustain.
        assert!(!Watchdog::evaluate(&hot_sample(), &si, 1_100, &mut watches, &cfg));
        assert_eq!(watches[SLOT].state, GpuState::Hot);
    }

    #[tokio::test]
    async fn disabled_watchdog_skips_evaluation_but_state_visible() {
        let (wd, _control, store, telemetry) = make_watchdog();
        wd.set_enabled(false);
        telemetry.poll_once().await;
        wd.tick().await; // no-op: no evaluation
        let evs = store.list_events(None, 100).unwrap();
        assert!(!evs.iter().any(|e| e.kind == "watchdog"));
        // Spec §4: disabled = no evaluation, state still visible.
        let st = wd.status().await;
        assert_eq!(st["enabled"], false);
        assert_eq!(st["hot"][SLOT]["temp_c"], 86);
        assert_eq!(st["hot"][SLOT]["threshold_c"], 90);
    }

    #[tokio::test]
    async fn unprivileged_watchdog_is_never_active_and_does_not_fire() {
        let store = Arc::new(Store::memory());
        let g = fake::gpu_sync(HOT_FILES);
        let control = Control::new(vec![g], store.clone());
        let telemetry = Telemetry::new(control.clone(), 600, None);
        // `enabled` defaults to true, but privileged=false ⇒ active=false.
        let wd =
            Watchdog::new_with(fast_cfg(), control, telemetry.clone(), store.clone(), false);
        assert!(wd.enabled(), "the user's intent is still on");
        assert!(!wd.active(), "not active without privilege");

        telemetry.poll_once().await;
        wd.tick().await; // would be Armed -> Hot if active
        tokio::time::sleep(Duration::from_millis(120)).await;
        telemetry.poll_once().await;
        wd.tick().await; // sustain elapsed — must NOT trigger

        // No trigger fired: no `watchdog` event.
        let evs = store.list_events(None, 100).unwrap();
        assert!(!evs.iter().any(|e| e.kind == "watchdog"));
        let st = wd.status().await;
        assert_eq!(st["enabled"], true);
        assert_eq!(st["active"], false);
        assert_eq!(st["privileged"], false);
    }

    #[tokio::test]
    async fn status_reports_hot_map_for_hot_gpu() {
        let (wd, _control, _store, telemetry) = make_watchdog();
        telemetry.poll_once().await;
        wd.tick().await;
        let st = wd.status().await;
        let hot = &st["hot"][SLOT];
        assert_eq!(hot["temp_c"], 86);
        assert_eq!(hot["threshold_c"], 90);
        assert!(hot["hot_for_ms"].is_u64());
        assert!(st["last_trigger"].is_null());
    }

    #[tokio::test]
    async fn watchdog_is_dormant_and_never_fires() {
        let (wd, _control, store, telemetry) = make_watchdog();
        // Armed + privileged by default, yet permanently dormant.
        assert!(wd.enabled(), "the stored intent is still on");
        assert!(!wd.active(), "dormant regardless of enabled + privileged");

        // Sustained heat across the fast sustain window — would trigger if
        // the watchdog were armed.
        telemetry.poll_once().await;
        wd.tick().await;
        tokio::time::sleep(Duration::from_millis(120)).await;
        telemetry.poll_once().await;
        wd.tick().await;

        // Dormant: no evaluation, no trigger, no audit trail.
        let evs = store.list_events(None, 100).unwrap();
        assert!(!evs.iter().any(|e| e.kind == "watchdog"));
        assert!(store.get_meta("watchdog_last_trigger").unwrap().is_none());
        let st = wd.status().await;
        assert_eq!(st["active"], false);
        assert!(st["last_trigger"].is_null());
    }

    #[test]
    fn safe_preset_id_roundtrips_through_meta() {
        let store = Arc::new(Store::memory());
        let g = fake::gpu_sync(HOT_FILES);
        let control = Control::new(vec![g], store.clone());
        let telemetry = Telemetry::new(control.clone(), 600, None);
        let wd = Watchdog::new_with(fast_cfg(), control, telemetry, store.clone(), true);
        wd.set_safe_preset(Some(42));
        assert_eq!(wd.safe_preset_id(), Some(42));
        assert_eq!(
            store.get_meta("watchdog_safe_preset").unwrap().as_deref(),
            Some("42")
        );
        wd.set_safe_preset(None);
        assert_eq!(wd.safe_preset_id(), None);
        assert_eq!(
            store.get_meta("watchdog_safe_preset").unwrap().as_deref(),
            Some("")
        );
    }
}
