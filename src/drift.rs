use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};

use crate::control::Control;
use crate::store::{self, now_ms, Store};
use crate::types::PresetSettings;

/// Bounded waits so a stuck apply cannot hang the guard (the old
/// watchdog's `APPLY_TIMEOUT` pattern).
const APPLY_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy)]
pub struct DriftGuardConfig {
    pub interval: Duration,
    /// Floor for the meta delay clamp. Production is 5 (the spec's 5–600
    /// range); tests inject 0 so a 1 s meta delay is usable.
    pub min_delay_s: i64,
}

impl DriftGuardConfig {
    /// Production constants: 5 s tick (`CHECK_INTERVAL`), 5 s delay floor.
    pub fn production() -> Self {
        Self {
            interval: Duration::from_secs(5),
            min_delay_s: 5,
        }
    }
}

#[derive(Clone)]
struct Pending {
    entry_id: i64,
    deadline_ms: i64,
}

pub struct DriftGuard {
    control: Arc<Control>,
    store: Arc<Store>,
    cfg: DriftGuardConfig,
    /// `std::sync::Mutex` with brief critical sections and no awaits inside
    /// (the watchdog/telemetry pattern).
    pending: std::sync::Mutex<BTreeMap<String, Pending>>,
}

impl DriftGuard {
    /// Production constructor (5 s tick, 5 s delay floor).
    pub fn new(control: Arc<Control>, store: Arc<Store>) -> Arc<Self> {
        Self::new_with(DriftGuardConfig::production(), control, store)
    }

    pub fn new_with(
        cfg: DriftGuardConfig,
        control: Arc<Control>,
        store: Arc<Store>,
    ) -> Arc<Self> {
        Arc::new(Self {
            control,
            store,
            cfg,
            pending: std::sync::Mutex::new(BTreeMap::new()),
        })
    }

    /// Spawn the background loop. The first `tokio::time::interval` tick
    /// completes immediately — `main` starts the guard only after
    /// `boot_reapply` so that first tick never logs a spurious episode
    /// (spec §5.7).
    pub fn start(self: &Arc<Self>) {
        let g = self.clone();
        let interval = self.cfg.interval;
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(interval);
            loop {
                tick.tick().await;
                g.tick().await;
            }
        });
    }

    /// One pass (spec §5.2): prune vanished slots, then per slot detect
    /// drift / advance the episode state machine.
    pub async fn tick(&self) {
        let live_slots: Vec<String> = self.control.gpus().iter().map(|g| g.slot.clone()).collect();

        // Prune: a slot that left the GPU set closes its pending entry
        // (spec §6.1) — otherwise the deadline would fire a no-op restore.
        {
            let mut p = self.pending.lock().unwrap();
            let stale: Vec<(String, i64)> = p
                .iter()
                .filter(|(slot, _)| !live_slots.contains(slot))
                .map(|(slot, e)| (slot.clone(), e.entry_id))
                .collect();
            for (slot, entry_id) in stale {
                self.store.set_drift_outcome(entry_id, "cleared", &None);
                p.remove(&slot);
            }
        }

        for g in self.control.gpus() {
            // Skip while an apply is in flight (spec §5.1 busy guard).
            if self.control.is_busy() {
                continue;
            }
            let live = crate::control::LiveReadback {
                power_cap_w: g.read_power_cap_w().await,
                od: g.read_od().await.ok(),
            };
            self.tick_slot(&g.slot, &live).await;
        }
    }

    /// `slot -> { entry_id, deadline_ms }` for `GET /api/drift`.
    pub fn pending_map(&self) -> Value {
        let p = self.pending.lock().unwrap();
        let mut m = serde_json::Map::new();
        for (slot, e) in p.iter() {
            m.insert(
                slot.clone(),
                json!({ "entry_id": e.entry_id, "deadline_ms": e.deadline_ms }),
            );
        }
        Value::Object(m)
    }

    /// Drop all pending restore timers (used by the history clear).
    pub fn clear_pending(&self) {
        self.pending.lock().unwrap().clear();
    }

    /// Whether the slot has a pending restore (for `GET /api/status`).
    pub fn is_drifted(&self, slot: &str) -> bool {
        self.pending.lock().unwrap().contains_key(slot)
    }

    /// The persisted auto-restore flag, read fresh each tick (spec §5.6).
    pub fn reapply_enabled(&self) -> bool {
        store::drift_reapply_from_meta(&self.store)
    }

    /// The persisted delay in seconds, read fresh each tick (spec §5.6).
    pub fn delay_s(&self) -> i64 {
        store::drift_delay_s_from_meta(&self.store).clamp(self.cfg.min_delay_s, 600)
    }

    async fn tick_slot(&self, slot: &str, live: &crate::control::LiveReadback) {
        let pending = self
            .pending
            .lock()
            .unwrap()
            .get(slot)
            .cloned();

        let Some(delta) = self.control.compare_drift(slot, live) else {
            // No drift (or busy/no-baseline — the tick already skipped those).
            // A pending episode whose drift self-cleared closes `cleared`.
            if let Some(p) = pending {
                self.pending.lock().unwrap().remove(slot);
                self.store.set_drift_outcome(p.entry_id, "cleared", &None);
            }
            return;
        };

        match pending {
            None => self.open_episode(slot, &delta.captured).await,
            Some(p) if now_ms() >= p.deadline_ms => {
                self.pending.lock().unwrap().remove(slot);
                self.fire_restore(slot, p.entry_id, &delta.captured).await;
            }
            Some(_) => {} // pending, before deadline, drift persists: nothing.
        }
    }

    /// Transition into drift: ledger entry + `external_drift` event
    /// (spec §5.3), then either a timer or an immediate close.
    async fn open_episode(&self, slot: &str, captured: &PresetSettings) {
        let previous_source = self
            .control
            .active_source(slot)
            .unwrap_or_else(|| "stock".into());
        let intended = self
            .intended_settings(slot)
            .and_then(|s| serde_json::to_value(&s).ok());
        let detected = json!({
            "power_cap_w": captured.power_cap_w,
            "sclk_offset_mhz": captured.sclk_offset_mhz,
            "max_mclk_mhz": captured.max_mclk_mhz,
            "voltage_offset_mv": captured.voltage_offset_mv,
        });
        let entry_id = match self
            .store
            .append_drift_entry(slot, &detected, &intended, &previous_source)
        {
            Ok(id) => id,
            Err(e) => {
                tracing::error!(slot, "drift ledger insert failed: {e}");
                return; // no ledger row → nothing to track this tick
            }
        };
        self.store.append_event(
            "external_drift",
            &json!({ "slot": slot, "previous_source": previous_source, "captured": captured }),
        );

        if self.reapply_enabled() && intended.is_some() {
            let deadline_ms = now_ms() + self.delay_s() * 1000;
            self.pending
                .lock()
                .unwrap()
                .insert(slot.to_string(), Pending { entry_id, deadline_ms });
        } else {
            // No intended state (`logged_only`) or re-apply off (`adopted`).
            // Adoption prevents entry-spam: expected == live from now on.
            let outcome = if intended.is_some() { "adopted" } else { "logged_only" };
            self.store.set_drift_outcome(entry_id, outcome, &None);
            self.control.adopt_external(slot, captured);
        }
    }

    /// Deadline reached with drift still present (spec §5.5).
    async fn fire_restore(&self, slot: &str, entry_id: i64, captured: &PresetSettings) {
        // Re-read the options at fire time (spec §5.6): re-apply may have
        // been disabled while the episode was pending.
        if !self.reapply_enabled() {
            self.store.set_drift_outcome(entry_id, "adopted", &None);
            self.control.adopt_external(slot, captured);
            return;
        }
        // The intended state may be gone (reverted to stock mid-episode):
        // never call apply_settings_blocking with a map lacking the slot —
        // an empty-matched apply reports `ok` and would record a false
        // `reapplied` (spec §5.5 step 1).
        let Some(settings) = self.intended_settings(slot) else {
            self.store.set_drift_outcome(entry_id, "logged_only", &None);
            self.control.adopt_external(slot, captured);
            return;
        };

        let map = BTreeMap::from([(slot.to_string(), self.restore_target(slot, &settings))]);
        let report = tokio::time::timeout(
            APPLY_TIMEOUT,
            self.control.apply_settings_blocking(&map, "drift_reapply"),
        )
        .await;
        match report {
            Ok(Ok(rep)) if rep.ok => {
                let detail = rep
                    .per_gpu
                    .get(slot)
                    .and_then(|o| serde_json::to_value(o).ok());
                self.store.set_drift_outcome(entry_id, "reapplied", &detail);
            }
            _ => {
                // Failure / timeout: a timeout can drop the apply mid-flight,
                // leaving DPM at `manual` — best-effort re-assert (spec §5.5
                // step 2).
                if let Some(g) = self.control.gpus().into_iter().find(|g| g.slot == slot) {
                    if let Err(e) = g.set_dpm_force("auto").await {
                        tracing::warn!(slot, "drift restore: DPM auto re-assert failed: {e}");
                    }
                }
                self.store.set_drift_outcome(entry_id, "reapply_failed", &None);
            }
        }
    }

    fn intended_settings(&self, slot: &str) -> Option<PresetSettings> {
        self.control
            .active_state()
            .and_then(|v| v.get("settings").cloned())
            .and_then(|s| serde_json::from_value::<BTreeMap<String, PresetSettings>>(s).ok())
            .and_then(|m| m.get(slot).cloned())
    }

    /// The restore target (spec §5.5 step 1): the full four-knob state —
    /// intended overlaid on the stock baseline — so a partial intended state
    /// plus an external change to an uncovered knob converges instead of
    /// looping. Degenerate baseline values (0.0 W / 0 MHz) stay `null`:
    /// writing them fails the driver's fail-closed range checks.
    fn restore_target(&self, slot: &str, settings: &PresetSettings) -> PresetSettings {
        let b = self.control.stock_baseline_slot(slot).unwrap_or(Value::Null);
        PresetSettings {
            power_cap_w: settings.power_cap_w.or_else(|| {
                b.get("power_cap_w")
                    .and_then(Value::as_f64)
                    .filter(|&w| w > 0.0)
            }),
            sclk_offset_mhz: settings.sclk_offset_mhz.or_else(|| {
                b.get("sclk_offset_mhz")
                    .and_then(Value::as_i64)
                    .map(|n| n as i32)
            }),
            max_mclk_mhz: settings.max_mclk_mhz.or_else(|| {
                b.get("max_mclk_mhz")
                    .and_then(Value::as_i64)
                    .filter(|&n| n > 0)
                    .map(|n| n as i32)
            }),
            voltage_offset_mv: settings.voltage_offset_mv.or_else(|| {
                b.get("voltage_offset_mv")
                    .and_then(Value::as_i64)
                    .map(|n| n as i32)
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::driver::fake::gpu as fake_gpu;
    use crate::driver::fake::gpu_with_slot;
    use crate::driver::fake::FakeSysfs;
    use crate::types::PresetSettings;
    use std::collections::BTreeMap;

    const OD_REL: &str = "card0/device/pp_od_clk_voltage";
    const CAP_REL: &str = "card0/device/hwmon/hwmon0/power1_cap";
    const DPM_REL: &str = "card0/device/power_dpm_force_performance_level";
    const SLOT: &str = "1002:75b4-1458:242f-0000:0b:00.0";
    const SLOT_B: &str = "1002:75b4-1458:242f-0000:0a:00.0";

    const OD: &str = "OD_SCLK_OFFSET:\n0Mhz\nOD_MCLK:\n0: 97Mhz\n1: 1258MHz\nOD_VDDGFX_OFFSET:\n0mV\nOD_RANGE:\nSCLK_OFFSET:    -500Mhz       1000Mhz\nMCLK:      97Mhz       1500Mhz\nVDDGFX_OFFSET:    -200mv          0mv";
    const FAN: &str = "OD_FAN_CURVE:\n0: 25C 30%\n1: 50C 50%\n2: 75C 75%\n3: 100C 100%\nOD_RANGE:\nFAN_CURVE(hotspot temp): 25C 100C\nFAN_CURVE(fan speed): 30% 100%";

    fn full_files() -> Vec<(&'static str, &'static str)> {
        vec![
            (OD_REL, OD),
            ("card0/device/gpu_od/fan_ctrl/fan_curve", FAN),
            (CAP_REL, "300000000"),
            ("card0/device/hwmon/hwmon0/power1_cap_min", "120000000"),
            ("card0/device/hwmon/hwmon0/power1_cap_max", "300000000"),
            (DPM_REL, "auto"),
        ]
    }

    fn test_cfg() -> DriftGuardConfig {
        DriftGuardConfig {
            interval: std::time::Duration::from_millis(50),
            min_delay_s: 0, // deadline tests use a 1 s meta delay
        }
    }

    /// Control + guard on a full fake tree with a captured baseline.
    async fn harness() -> (Arc<Control>, Arc<Store>, Arc<DriftGuard>) {
        let store = Arc::new(Store::memory());
        let g = fake_gpu(&full_files()).await;
        let control = Control::new(vec![g], store.clone());
        control.capture_baseline().await.unwrap();
        let guard = DriftGuard::new_with(test_cfg(), control.clone(), store.clone());
        (control, store, guard)
    }

    async fn apply_full(control: &Arc<Control>) {
        control
            .apply_settings(
                &BTreeMap::from([(
                    SLOT.to_string(),
                    PresetSettings {
                        power_cap_w: Some(235.0),
                        sclk_offset_mhz: Some(100),
                        max_mclk_mhz: Some(1150),
                        voltage_offset_mv: Some(-50),
                        ..Default::default()
                    },
                )]),
                "adhoc",
            )
            .await
            .unwrap();
    }

    /// Out-of-band stock reset, like a card that rebooted itself: the fake
    /// OD table back to stock (staged grammar: `s`/`m`/`vo`/`c`) and the
    /// power cap to 300 W.
    async fn drift_to_stock(control: &Arc<Control>) {
        let g = &control.gpus()[0];
        g.root.write(OD_REL, "m 1 1258").await.unwrap();
        g.root.write(OD_REL, "s 0").await.unwrap();
        g.root.write(OD_REL, "vo 0").await.unwrap();
        g.root.write(OD_REL, "c").await.unwrap();
        g.root.write(CAP_REL, "300000000").await.unwrap();
    }

    fn entries(store: &Arc<Store>) -> Vec<crate::types::DriftEntry> {
        store.list_drift_entries().unwrap()
    }

    /// Poll ticks until the latest entry is no longer `pending`. The test
    /// must set `drift_reapply_delay_s` to "1" BEFORE the episode-opening
    /// tick — a pending timer keeps its original deadline (spec §5.6), so
    /// setting it later would leave the deadline at the 30 s default.
    async fn tick_past_deadline(guard: &Arc<DriftGuard>, store: &Arc<Store>) {
        let start = std::time::Instant::now();
        loop {
            guard.tick().await;
            if store.list_drift_entries().unwrap().iter().all(|e| e.outcome != "pending") {
                return;
            }
            assert!(start.elapsed() < std::time::Duration::from_secs(5), "deadline never fired");
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    }

    #[tokio::test]
    async fn detects_drift_one_entry_one_timer_per_episode() {
        let (control, store, guard) = harness().await;
        apply_full(&control).await;
        // The API layer normally sets the source label; do it here so the
        // ledger's `previous_source` is meaningful.
        store.set_active_source(SLOT, "adhoc");
        drift_to_stock(&control).await;
        guard.tick().await;
        assert_eq!(entries(&store).len(), 1);
        assert_eq!(entries(&store)[0].outcome, "pending");
        assert_eq!(entries(&store)[0].previous_source, "adhoc");
        assert_eq!(entries(&store)[0].intended,
            Some(serde_json::json!({"power_cap_w": 235.0, "sclk_offset_mhz": 100, "max_mclk_mhz": 1150, "voltage_offset_mv": -50})));
        assert!(guard.pending_map().get(SLOT).is_some());
        // Repeated ticks while drifted add nothing.
        guard.tick().await;
        guard.tick().await;
        assert_eq!(entries(&store).len(), 1);
        // Exactly one external_drift event per episode.
        let evs = store.list_events(None, 100).unwrap();
        assert_eq!(evs.iter().filter(|e| e.kind == "external_drift").count(), 1);
    }

    #[tokio::test]
    async fn deadline_fire_reapplies_single_slot() {
        let (control, store, guard) = harness().await;
        apply_full(&control).await;
        store.set_active_source(SLOT, "adhoc");
        store.set_meta("drift_reapply_delay_s", "1"); // before the opening tick
        drift_to_stock(&control).await;
        guard.tick().await;
        tick_past_deadline(&guard, &store).await;
        let e = &entries(&store)[0];
        assert_eq!(e.outcome, "reapplied");
        assert!(e.outcome_detail.is_some());
        // Hardware back at the intended values.
        let g = &control.gpus()[0];
        assert!((g.read_power_cap_w().await.unwrap() - 235.0).abs() < 1.0);
        let od = g.read_od().await.unwrap();
        assert_eq!(od.sclk_offset_mhz, 100);
        assert_eq!(od.mclk.get(&1).copied(), Some(1150));
        assert_eq!(od.vddgfx_mv, -50);
        assert!(guard.pending_map().as_object().unwrap().is_empty());
        // The slot keeps its original source label (no relabel by restore).
        assert_eq!(control.active_source(SLOT).as_deref(), Some("adhoc"));
    }

    /// A Sysfs whose write() fails when `content` contains `fail_on`
    /// (duplicated from control.rs tests — that one is private there).
    #[derive(Clone)]
    struct FailSysfs {
        inner: FakeSysfs,
        fail_on: String,
    }
    #[async_trait::async_trait]
    impl crate::driver::sysfs::Sysfs for FailSysfs {
        async fn read(&self, rel: &str) -> anyhow::Result<String> {
            self.inner.read(rel).await
        }
        async fn write(&self, rel: &str, content: &str) -> anyhow::Result<()> {
            if content.contains(&self.fail_on) {
                anyhow::bail!("injected failure ({content})");
            }
            self.inner.write(rel, content).await
        }
        async fn exists(&self, rel: &str) -> bool {
            self.inner.exists(rel).await
        }
        async fn read_all(&self, rel: &str) -> anyhow::Result<String> {
            self.inner.read_all(rel).await
        }
        async fn list_dir(&self, rel: &str) -> anyhow::Result<Vec<String>> {
            self.inner.list_dir(rel).await
        }
    }

    #[tokio::test]
    async fn apply_failure_reapply_failed_then_fresh_episode() {
        let store = Arc::new(Store::memory());
        // Power-cap writes of "235" (i.e. 235000000 µW) fail.
        let inner = FakeSysfs::with(&full_files());
        let bad = Arc::new(FailSysfs { inner, fail_on: "235000000".into() });
        let g = crate::driver::Gpu {
            root: bad,
            rel: "card0".into(),
            slot: SLOT.into(),
            card_n: 0,
            hwmon: "hwmon0".into(),
        };
        let control = Control::new(vec![g], store.clone());
        control.capture_baseline().await.unwrap();
        apply_full(&control).await;
        let guard = DriftGuard::new_with(test_cfg(), control.clone(), store.clone());
        store.set_meta("drift_reapply_delay_s", "1"); // before the opening tick
        drift_to_stock(&control).await;
        guard.tick().await;
        tick_past_deadline(&guard, &store).await;
        assert_eq!(entries(&store)[0].outcome, "reapply_failed");
        // Drift still present → the next tick opens a fresh episode.
        guard.tick().await;
        assert_eq!(entries(&store).len(), 2);
        assert_eq!(entries(&store)[0].outcome, "pending");
    }

    #[tokio::test]
    async fn self_clear_before_deadline_cleared() {
        let (control, store, guard) = harness().await;
        apply_full(&control).await;
        drift_to_stock(&control).await;
        guard.tick().await;
        assert_eq!(entries(&store)[0].outcome, "pending");
        // External tool reverts to the intended values before the deadline.
        apply_full(&control).await;
        guard.tick().await;
        assert_eq!(entries(&store).len(), 1);
        assert_eq!(entries(&store)[0].outcome, "cleared");
        assert!(guard.pending_map().as_object().unwrap().is_empty());
    }

    #[tokio::test]
    async fn stock_slot_logged_only_and_values_adopted() {
        let (control, store, guard) = harness().await;
        // No intended state (nothing applied).
        drift_to_stock(&control).await;
        // Stock OD == stock baseline: no drift on OD; drift the cap only.
        // (drift_to_stock already set 300 W == baseline, so write 280 W.)
        control.gpus()[0].root.write(CAP_REL, "280000000").await.unwrap();
        guard.tick().await;
        assert_eq!(entries(&store).len(), 1);
        assert_eq!(entries(&store)[0].outcome, "logged_only");
        assert_eq!(entries(&store)[0].intended, None);
        // Live values adopted as external → the next tick is clean.
        assert_eq!(control.active_source(SLOT).as_deref(), Some("external"));
        assert!(guard.pending_map().as_object().unwrap().is_empty());
        guard.tick().await;
        assert_eq!(entries(&store).len(), 1);
    }

    #[tokio::test]
    async fn reapply_disabled_adopted() {
        let (control, store, guard) = harness().await;
        apply_full(&control).await;
        store.set_meta("drift_reapply", "false");
        drift_to_stock(&control).await;
        guard.tick().await;
        assert_eq!(entries(&store)[0].outcome, "adopted");
        assert_eq!(control.active_source(SLOT).as_deref(), Some("external"));
        assert_eq!(
            control.active_state().unwrap()["settings"][SLOT]["max_mclk_mhz"],
            1258
        );
        assert!(guard.pending_map().as_object().unwrap().is_empty());
    }

    #[tokio::test]
    async fn re_drift_after_restore_new_episode() {
        let (control, store, guard) = harness().await;
        apply_full(&control).await;
        store.set_meta("drift_reapply_delay_s", "1"); // before the opening tick
        drift_to_stock(&control).await;
        guard.tick().await;
        tick_past_deadline(&guard, &store).await;
        assert_eq!(entries(&store)[0].outcome, "reapplied");
        // Drift again.
        drift_to_stock(&control).await;
        guard.tick().await;
        assert_eq!(entries(&store).len(), 2);
        assert_eq!(entries(&store)[0].outcome, "pending");
    }

    #[tokio::test]
    async fn busy_tick_skipped_no_false_episode() {
        let (control, store, guard) = harness().await;
        apply_full(&control).await;
        drift_to_stock(&control).await;
        let _guard = control.busy.try_lock().unwrap();
        guard.tick().await;
        assert!(entries(&store).is_empty());
        assert!(guard.pending_map().as_object().unwrap().is_empty());
    }

    #[tokio::test]
    async fn no_baseline_slot_skipped() {
        let store = Arc::new(Store::memory());
        let g = fake_gpu(&full_files()).await;
        let control = Control::new(vec![g], store.clone());
        // No capture_baseline.
        let guard = DriftGuard::new_with(test_cfg(), control.clone(), store.clone());
        control.gpus()[0].root.write(CAP_REL, "280000000").await.unwrap();
        guard.tick().await;
        assert!(entries(&store).is_empty());
    }

    #[tokio::test]
    async fn slot_removed_prunes_pending() {
        let store = Arc::new(Store::memory());
        let ga = fake_gpu(&full_files()).await;
        let gb = gpu_with_slot(&full_files(), SLOT_B);
        let control = Control::new(vec![ga, gb], store.clone());
        control.capture_baseline().await.unwrap();
        let guard = DriftGuard::new_with(test_cfg(), control.clone(), store.clone());
        control
            .apply_settings(
                &BTreeMap::from([(
                    SLOT.to_string(),
                    PresetSettings { power_cap_w: Some(235.0), ..Default::default() },
                )]),
                "adhoc",
            )
            .await
            .unwrap();
        control.gpus()[0].root.write(CAP_REL, "300000000").await.unwrap();
        guard.tick().await;
        assert_eq!(entries(&store).len(), 1);
        // Slot A leaves the GPU set (uevent removal).
        let gb = control.gpus().into_iter().find(|g| g.slot == SLOT_B).unwrap();
        control.set_gpus(vec![gb]);
        guard.tick().await;
        assert_eq!(entries(&store)[0].outcome, "cleared");
        assert!(guard.pending_map().as_object().unwrap().is_empty());
    }

    #[tokio::test]
    async fn clear_pending_empties_map() {
        let (control, _store, guard) = harness().await;
        apply_full(&control).await;
        drift_to_stock(&control).await;
        guard.tick().await;
        assert!(guard.pending_map().get(SLOT).is_some());
        guard.clear_pending();
        assert!(guard.pending_map().as_object().unwrap().is_empty());
    }

    #[tokio::test]
    async fn reverted_mid_episode_deadline_logged_only() {
        // The non-root dev case (spec §5.5 step 1): the user's revert drops
        // the intended state, but its stock writes are rejected by the OS
        // (injected here), so the hardware stays drifted. The deadline must
        // close `logged_only` + adopt — never a no-op `reapplied`.
        let store = Arc::new(Store::memory());
        // The revert's stock power-cap write ("300000000") fails.
        let inner = FakeSysfs::with(&full_files());
        let bad = Arc::new(FailSysfs { inner, fail_on: "300000000".into() });
        let g = crate::driver::Gpu {
            root: bad,
            rel: "card0".into(),
            slot: SLOT.into(),
            card_n: 0,
            hwmon: "hwmon0".into(),
        };
        let control = Control::new(vec![g], store.clone());
        control.capture_baseline().await.unwrap();
        apply_full(&control).await; // writes 235000000 — not the failing value
        let guard = DriftGuard::new_with(test_cfg(), control.clone(), store.clone());
        store.set_meta("drift_reapply_delay_s", "1"); // before the opening tick
        // Drift the cap to 280 W (an out-of-band write that succeeds).
        control.gpus()[0].root.write(CAP_REL, "280000000").await.unwrap();
        guard.tick().await;
        assert_eq!(entries(&store)[0].outcome, "pending");
        // Revert: the intended state is dropped even though the write fails.
        control.revert_for_slot(SLOT).await.unwrap();
        assert!(control
            .active_state()
            .unwrap()
            .get("settings")
            .map(|s| s.get(SLOT).is_none())
            .unwrap_or(true));
        tick_past_deadline(&guard, &store).await;
        assert_eq!(entries(&store)[0].outcome, "logged_only");
        assert_eq!(control.active_source(SLOT).as_deref(), Some("external"));
        // Adopted: the next tick is clean (no new episode).
        guard.tick().await;
        assert_eq!(entries(&store).len(), 1);
    }

    #[tokio::test]
    async fn reapply_disabled_while_pending_adopted() {
        let (control, store, guard) = harness().await;
        apply_full(&control).await;
        store.set_meta("drift_reapply_delay_s", "1"); // before the opening tick
        drift_to_stock(&control).await;
        guard.tick().await;
        assert_eq!(entries(&store)[0].outcome, "pending");
        store.set_meta("drift_reapply", "false");
        tick_past_deadline(&guard, &store).await;
        assert_eq!(entries(&store)[0].outcome, "adopted");
        assert_eq!(control.active_source(SLOT).as_deref(), Some("external"));
    }

    #[tokio::test]
    async fn partial_intended_state_converges() {
        let (control, store, guard) = harness().await;
        // Intended state covers ONLY the power cap.
        control
            .apply_settings(
                &BTreeMap::from([(
                    SLOT.to_string(),
                    PresetSettings { power_cap_w: Some(235.0), ..Default::default() },
                )]),
                "adhoc",
            )
            .await
            .unwrap();
        store.set_meta("drift_reapply_delay_s", "1"); // before the opening tick
        // Third-party tool changes an uncovered knob (sclk offset).
        let g = &control.gpus()[0];
        g.root.write(OD_REL, "s 100").await.unwrap();
        g.root.write(OD_REL, "c").await.unwrap();
        guard.tick().await;
        assert_eq!(entries(&store).len(), 1);
        tick_past_deadline(&guard, &store).await;
        assert_eq!(entries(&store)[0].outcome, "reapplied");
        // Uncovered knob back at its stock baseline (0) → next tick is clean.
        let od = g.read_od().await.unwrap();
        assert_eq!(od.sclk_offset_mhz, 0);
        guard.tick().await;
        assert_eq!(entries(&store).len(), 1, "no repeated episodes");
    }

    #[tokio::test]
    async fn delay_and_flag_from_meta() {
        let (_control, store, guard) = harness().await;
        assert!(guard.reapply_enabled()); // absent → on
        assert_eq!(guard.delay_s(), 30); // absent → 30
        store.set_meta("drift_reapply", "false");
        assert!(!guard.reapply_enabled());
        store.set_meta("drift_reapply_delay_s", "abc");
        assert_eq!(guard.delay_s(), 30);
        store.set_meta("drift_reapply_delay_s", "1");
        assert_eq!(guard.delay_s(), 1); // min_delay_s = 0 in tests
        store.set_meta("drift_reapply_delay_s", "601");
        assert_eq!(guard.delay_s(), 600);
    }

    /// Spec §8.4: the full loop through the *spawned* tick task — apply a
    /// setting, reset the fake hardware to stock out-of-band, assert a ledger
    /// entry appears and, after the (short) delay, the fake hardware holds
    /// the intended values again.
    #[tokio::test]
    async fn e2e_spawned_loop_detects_and_restores() {
        let (control, store, guard) = harness().await;
        apply_full(&control).await;
        store.set_meta("drift_reapply_delay_s", "1");
        guard.start(); // 50 ms tick interval from test_cfg()
        // Let the first (immediate) tick run against a clean card: no entry.
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        assert!(entries(&store).is_empty(), "clean card must not log");
        // Out-of-band stock reset.
        drift_to_stock(&control).await;
        // Detection within a tick, restore within the 1 s delay.
        let start = std::time::Instant::now();
        loop {
            let rows = entries(&store);
            if rows.len() == 1 && rows[0].outcome == "reapplied" {
                break;
            }
            assert!(
                start.elapsed() < std::time::Duration::from_secs(10),
                "e2e never completed: {rows:?}"
            );
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        let g = &control.gpus()[0];
        assert!((g.read_power_cap_w().await.unwrap() - 235.0).abs() < 1.0);
        let od = g.read_od().await.unwrap();
        assert_eq!(od.mclk.get(&1).copied(), Some(1150));
        assert!(guard.pending_map().as_object().unwrap().is_empty());
    }
}
