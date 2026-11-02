use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{broadcast, RwLock};

use crate::control::Control;
use crate::driver::read::FastFields;
use crate::types::GpuSample;

/// Per-slot fold of one period's fast sub-samples (spec §3.2).
/// max for temps/power, integer mean (sum+count) for the slow-moving fields.
#[derive(Debug, Clone, Default)]
pub struct FastAccumulator {
    n: u32,
    socket_power_max: Option<f64>,
    edge_max: Option<i32>,
    hotspot_max: Option<i32>,
    vram_max: Option<i32>,
    gfx_mhz_sum: i64,
    gfx_mhz_n: u32,
    mem_mhz_sum: i64,
    mem_mhz_n: u32,
    gfx_act_sum: i64,
    gfx_act_n: u32,
    mem_act_sum: i64,
    mem_act_n: u32,
    vram_used_sum: i64,
    vram_used_n: u32,
    dpm_sclk_sum: i64,
    dpm_sclk_n: u32,
    dpm_mclk_sum: i64,
    dpm_mclk_n: u32,
}

impl FastAccumulator {
    fn fold(&mut self, f: &FastFields) {
        self.n += 1;
        if let Some(v) = f.socket_power_w {
            self.socket_power_max = Some(self.socket_power_max.map(|m| m.max(v)).unwrap_or(v));
        }
        if let Some(v) = f.edge_c {
            self.edge_max = Some(self.edge_max.map(|m| m.max(v)).unwrap_or(v));
        }
        if let Some(v) = f.hotspot_c {
            self.hotspot_max = Some(self.hotspot_max.map(|m| m.max(v)).unwrap_or(v));
        }
        if let Some(v) = f.vram_c {
            self.vram_max = Some(self.vram_max.map(|m| m.max(v)).unwrap_or(v));
        }
        if let Some(v) = f.gfx_mhz {
            self.gfx_mhz_sum += v as i64;
            self.gfx_mhz_n += 1;
        }
        if let Some(v) = f.mem_mhz {
            self.mem_mhz_sum += v as i64;
            self.mem_mhz_n += 1;
        }
        if let Some(v) = f.gfx_activity_pct {
            self.gfx_act_sum += v as i64;
            self.gfx_act_n += 1;
        }
        if let Some(v) = f.mem_activity_pct {
            self.mem_act_sum += v as i64;
            self.mem_act_n += 1;
        }
        if let Some(v) = f.dpm_sclk_mhz {
            self.dpm_sclk_sum += v as i64;
            self.dpm_sclk_n += 1;
        }
        if let Some(v) = f.dpm_mclk_mhz {
            self.dpm_mclk_sum += v as i64;
            self.dpm_mclk_n += 1;
        }
        if let Some(v) = f.vram_used_mb {
            self.vram_used_sum += v;
            self.vram_used_n += 1;
        }
    }

    fn is_empty(&self) -> bool {
        self.n == 0
    }

    fn commit(self) -> FastFields {
        FastFields {
            socket_power_w: self.socket_power_max,
            edge_c: self.edge_max,
            hotspot_c: self.hotspot_max,
            vram_c: self.vram_max,
            gfx_mhz: (self.gfx_mhz_n > 0)
                .then(|| (self.gfx_mhz_sum / self.gfx_mhz_n as i64) as i32),
            mem_mhz: (self.mem_mhz_n > 0)
                .then(|| (self.mem_mhz_sum / self.mem_mhz_n as i64) as i32),
            gfx_activity_pct: (self.gfx_act_n > 0)
                .then(|| (self.gfx_act_sum / self.gfx_act_n as i64) as i32),
            mem_activity_pct: (self.mem_act_n > 0)
                .then(|| (self.mem_act_sum / self.mem_act_n as i64) as i32),
            vram_used_mb: (self.vram_used_n > 0)
                .then(|| self.vram_used_sum / self.vram_used_n as i64),
            dpm_sclk_mhz: (self.dpm_sclk_n > 0)
                .then(|| (self.dpm_sclk_sum / self.dpm_sclk_n as i64) as i32),
            dpm_mclk_mhz: (self.dpm_mclk_n > 0)
                .then(|| (self.dpm_mclk_sum / self.dpm_mclk_n as i64) as i32),
        }
    }
}

pub struct Telemetry {
    control: Arc<Control>,
    ring_seconds: std::sync::atomic::AtomicU64,
    /// Real sysfs root for the best-effort `gpu_metrics` read; `None` in
    /// tests (no test touches real hardware).
    metrics_root: Option<String>,
    latest: RwLock<BTreeMap<String, GpuSample>>,
    ring: RwLock<Vec<GpuSample>>,
    tx: broadcast::Sender<GpuSample>,
    /// Per-slot period accumulators. std Mutex: the critical section is
    /// read-then-reset with no `.await` inside (spec §3.1).
    accumulators: std::sync::Mutex<BTreeMap<String, FastAccumulator>>,
}

impl Telemetry {
    pub fn new(
        control: Arc<Control>,
        ring_seconds: u64,
        metrics_root: Option<String>,
    ) -> Arc<Self> {
        let (tx, _) = broadcast::channel(64);
        Arc::new(Self {
            control,
            ring_seconds: std::sync::atomic::AtomicU64::new(ring_seconds),
            metrics_root,
            latest: RwLock::new(BTreeMap::new()),
            ring: RwLock::new(Vec::new()),
            tx,
            accumulators: std::sync::Mutex::new(BTreeMap::new()),
        })
    }

    /// One poll iteration: every GPU, best-effort.
    pub async fn poll_once(&self) {
        let mut ring_guard = self.ring.write().await;
        for g in self.control.gpus() {
            let slow = g.read_slow_fields().await;
            // Best-effort gpu_metrics (throttle bits, fan, voltages, PCIe) + ECC.
            // Only for real sysfs roots — skipped entirely in tests.
            let (extra, ecc) = if let Some(root) = self.metrics_root.clone() {
                let rel = g.rel.clone();
                tokio::task::spawn_blocking(move || {
                    (
                        crate::driver::read::gpu_metrics_for(&root, &rel),
                        crate::driver::read::ecc_for(&root, &rel),
                    )
                })
                .await
                .unwrap_or_default()
            } else {
                (Default::default(), None)
            };
            // Atomic read-then-reset under the accumulator lock (spec §3.1).
            // Empty (no sub-samples this period, e.g. record_s == 0) → one
            // fresh fast read, preserving today's single-cadence behavior.
            let acc = self.accumulators.lock().unwrap().remove(&g.slot);
            let fast = match acc {
                Some(a) if !a.is_empty() => a.commit(),
                _ => match g.read_fast_fields().await {
                    Ok(f) => f,
                    Err(e) => {
                        tracing::warn!(slot = %g.slot, "telemetry read failed: {e}");
                        continue;
                    }
                },
            };
            let sample = GpuSample {
                slot: g.slot.clone(),
                ts_ms: crate::store::now_ms(),
                socket_power_w: fast.socket_power_w.unwrap_or(0.0),
                power_cap_w: slow.power_cap_w,
                edge_c: fast.edge_c,
                hotspot_c: fast.hotspot_c,
                vram_c: fast.vram_c,
                gfx_mhz: fast.gfx_mhz,
                mem_mhz: fast.mem_mhz,
                gfx_activity_pct: fast.gfx_activity_pct,
                mem_activity_pct: fast.mem_activity_pct,
                vram_used_mb: fast.vram_used_mb,
                vram_total_mb: slow.vram_total_mb,
                dpm_sclk_mhz: fast.dpm_sclk_mhz,
                dpm_mclk_mhz: fast.dpm_mclk_mhz,
                throttle_bits: extra.throttle,
                fan_rpm: extra.fan_rpm,
                fan_pwm_pct: extra.fan_pwm,
                voltage_gfx_mv: extra.voltage_gfx_mv,
                voltage_soc_mv: extra.voltage_soc_mv,
                voltage_mem_mv: extra.voltage_mem_mv,
                ecc_umc_ce: ecc.as_ref().map(|c| c.ce),
                ecc_umc_ue: ecc.as_ref().map(|c| c.ue),
                ecc_umc_deferred: ecc.as_ref().and_then(|c| c.deferred),
                pcie_width: extra.pcie_width,
                pcie_speed_gt: extra.pcie_speed_gt,
            };
            self.latest
                .write()
                .await
                .insert(sample.slot.clone(), sample.clone());
            ring_guard.push(sample.clone());
            let _ = self.tx.send(sample);
        }
        // Trim ring to the window (live-updatable; spec §4.2).
        let window_s = self.ring_seconds.load(std::sync::atomic::Ordering::Relaxed);
        let cutoff_ms = crate::store::now_ms().saturating_sub((window_s * 1000) as i64);
        ring_guard.retain(|s| s.ts_ms >= cutoff_ms);
    }

    pub fn start(self: &Arc<Self>, interval: Duration) {
        let t = self.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(interval);
            // First `tick()` on a fresh interval completes immediately.
            loop {
                tick.tick().await;
                t.poll_once().await;
            }
        });
    }

    /// One fast tick: read the cheap fields per GPU and fold into the
    /// period accumulator. Best-effort — a failed read is skipped (the
    /// reduction skips `None` sub-samples).
    pub async fn record_once(&self) {
        for g in self.control.gpus() {
            match g.read_fast_fields().await {
                Ok(f) => {
                    let mut accs = self.accumulators.lock().unwrap();
                    accs.entry(g.slot.clone()).or_default().fold(&f);
                }
                Err(e) => tracing::warn!(slot = %g.slot, "fast telemetry read failed: {e}"),
            }
        }
    }

    pub fn start_fast(self: &Arc<Self>, interval: Duration) {
        let t = self.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(interval);
            loop {
                tick.tick().await;
                t.record_once().await;
            }
        });
    }

    pub async fn latest(&self) -> BTreeMap<String, GpuSample> {
        self.latest.read().await.clone()
    }

    pub async fn ring(&self) -> Vec<GpuSample> {
        self.ring.read().await.clone()
    }

    pub fn set_ring_seconds(&self, s: u64) {
        self.ring_seconds.store(s, std::sync::atomic::Ordering::Relaxed);
    }

    /// Spec-mandated accessor (design spec §4.2); exercised in tests.
    #[allow(dead_code)]
    pub fn ring_seconds(&self) -> u64 {
        self.ring_seconds.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn subscribe(&self) -> broadcast::Receiver<GpuSample> {
        self.tx.subscribe()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::Control;
    use crate::driver::fake;
    use crate::driver::read::FastFields;
    use crate::driver::sysfs::MemSysfs;
    use crate::driver::Gpu;
    use crate::store::Store;
    use std::sync::Arc;

    const SAMPLE_FILES: &[(&str, &str)] = &[
        ("card0/device/hwmon/hwmon0/power1_average", "299000000"),
        ("card0/device/gpu_busy_percent", "100"),
    ];

    fn tele() -> (Arc<Telemetry>, Arc<Control>) {
        let store = Arc::new(Store::memory());
        let g = fake::gpu_sync(SAMPLE_FILES);
        // Control::new already returns Arc<Control>; metrics_root None = no gpu_metrics in tests.
        let control = Control::new(vec![g], store);
        (Telemetry::new(control.clone(), 600, None), control)
    }

    #[test]
    fn ring_seconds_initializes_from_new_and_round_trips() {
        let (t, _) = tele();
        assert_eq!(t.ring_seconds(), 600);
        t.set_ring_seconds(900);
        assert_eq!(t.ring_seconds(), 900);
    }

    #[tokio::test]
    async fn poll_populates_latest_and_ring() {
        let (t, _) = tele();
        t.poll_once().await;
        let latest = t.latest().await;
        assert_eq!(latest.len(), 1);
        let s = &latest["1002:75b4-1458:242f-0000:0b:00.0"];
        assert_eq!(s.socket_power_w, 299.0);
        let ring = t.ring().await;
        assert_eq!(ring.len(), 1);
    }

    #[tokio::test]
    async fn broadcast_receives_sample() {
        let (t, _) = tele();
        let mut rx = t.subscribe();
        t.poll_once().await;
        let s = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(s.socket_power_w, 299.0);
    }

    #[tokio::test]
    async fn poll_fills_ecc_from_metrics_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_string_lossy().to_string();
        std::fs::create_dir_all(dir.path().join("card0/device/ras")).unwrap();
        std::fs::write(
            dir.path().join("card0/device/ras/umc_err_count"),
            "ue: 9\nce: 7\nde: 1\n",
        )
        .unwrap();
        let store = Arc::new(Store::memory());
        let g = fake::gpu_sync(SAMPLE_FILES);
        let control = Control::new(vec![g], store);
        let t = Telemetry::new(control.clone(), 600, Some(root));
        t.poll_once().await;
        let latest = t.latest().await;
        let s = &latest["1002:75b4-1458:242f-0000:0b:00.0"];
        assert_eq!(s.ecc_umc_ce, Some(7));
        assert_eq!(s.ecc_umc_ue, Some(9));
        assert_eq!(s.ecc_umc_deferred, Some(1));
    }

    #[test]
    fn accumulator_max_captures_peak() {
        let mut a = FastAccumulator::default();
        a.fold(&FastFields { hotspot_c: Some(70), ..Default::default() });
        a.fold(&FastFields { hotspot_c: Some(95), ..Default::default() });
        a.fold(&FastFields { hotspot_c: Some(72), ..Default::default() });
        let f = a.commit();
        assert_eq!(f.hotspot_c, Some(95));
    }

    #[test]
    fn accumulator_avg_is_period_mean() {
        let mut a = FastAccumulator::default();
        a.fold(&FastFields {
            gfx_mhz: Some(1000),
            vram_used_mb: Some(1000),
            ..Default::default()
        });
        a.fold(&FastFields {
            gfx_mhz: Some(2000),
            vram_used_mb: Some(2000),
            ..Default::default()
        });
        let f = a.commit();
        assert_eq!(f.gfx_mhz, Some(1500));
        assert_eq!(f.vram_used_mb, Some(1500));
    }

    #[test]
    fn accumulator_skips_none_subsamples() {
        let mut a = FastAccumulator::default();
        a.fold(&FastFields { edge_c: None, gfx_mhz: Some(1000), ..Default::default() });
        a.fold(&FastFields { edge_c: Some(80), gfx_mhz: None, ..Default::default() });
        let f = a.commit();
        assert_eq!(f.edge_c, Some(80));
        assert_eq!(f.gfx_mhz, Some(1000));
    }

    #[test]
    fn accumulator_all_none_field_stays_none() {
        let mut a = FastAccumulator::default();
        a.fold(&FastFields::default());
        a.fold(&FastFields::default());
        let f = a.commit();
        assert_eq!(f.edge_c, None);
        assert_eq!(f.socket_power_w, None);
    }

    #[test]
    fn accumulator_single_subsample_is_identity() {
        let f = FastFields {
            socket_power_w: Some(250.5),
            edge_c: Some(77),
            gfx_mhz: Some(3110),
            vram_used_mb: Some(8192),
            ..Default::default()
        };
        let mut a = FastAccumulator::default();
        a.fold(&f);
        assert_eq!(a.commit(), f);
    }

    #[test]
    fn accumulator_empty_period_is_all_none() {
        let a = FastAccumulator::default();
        assert!(a.is_empty());
        assert_eq!(a.commit(), FastFields::default());
    }

    async fn tele_with_root() -> (Arc<Telemetry>, Arc<MemSysfs>) {
        let s = Arc::new(MemSysfs::new());
        let h = "card0/device/hwmon/hwmon0";
        s.put(&format!("{h}/power1_average"), "299000000").await;
        s.put(&format!("{h}/temp1_label"), "edge").await;
        s.put(&format!("{h}/temp1_input"), "70000").await;
        s.put(&format!("{h}/temp2_label"), "junction").await;
        s.put(&format!("{h}/temp2_input"), "70000").await;
        s.put(&format!("{h}/temp3_label"), "mem").await;
        s.put(&format!("{h}/temp3_input"), "60000").await;
        s.put("card0/device/gpu_busy_percent", "100").await;
        let g = Gpu {
            root: s.clone(),
            rel: "card0".into(),
            slot: "1002:75b4-1458:242f-0000:0b:00.0".into(),
            card_n: 0,
            hwmon: "hwmon0".into(),
        };
        let control = Control::new(vec![g], Arc::new(Store::memory()));
        (Telemetry::new(control, 600, None), s)
    }

    const SLOT: &str = "1002:75b4-1458:242f-0000:0b:00.0";

    #[tokio::test]
    async fn spike_is_captured_as_period_peak() {
        let (t, root) = tele_with_root().await;
        // Three sub-samples; the middle one spikes the hotspot AND power.
        t.record_once().await;
        root.put("card0/device/hwmon/hwmon0/temp2_input", "95000").await;
        root.put("card0/device/hwmon/hwmon0/power1_average", "350000000").await;
        t.record_once().await;
        root.put("card0/device/hwmon/hwmon0/temp2_input", "71000").await;
        root.put("card0/device/hwmon/hwmon0/power1_average", "299000000").await;
        t.record_once().await;
        t.poll_once().await;
        let latest = t.latest().await;
        assert_eq!(latest[SLOT].hotspot_c, Some(95)); // peak, not the last (71)
        // Power: 299 / 350 / 299 W → max = 350.0 (spec §4: same shape for power).
        assert_eq!(latest[SLOT].socket_power_w, 350.0);
    }

    #[tokio::test]
    async fn second_period_starts_clean_after_commit() {
        let (t, root) = tele_with_root().await;
        t.record_once().await;
        t.poll_once().await; // period 1 committed, accumulator reset
        // Period 2: power drops; a stale period-1 value must not leak in.
        root.put("card0/device/hwmon/hwmon0/power1_average", "100000000").await;
        t.record_once().await;
        t.poll_once().await;
        let ring = t.ring().await;
        assert_eq!(ring.len(), 2);
        assert_eq!(ring[0].socket_power_w, 299.0);
        assert_eq!(ring[1].socket_power_w, 100.0);
    }

    #[tokio::test]
    async fn empty_period_falls_back_to_single_fast_read() {
        let (t, _) = tele_with_root().await;
        // No record_once calls: commit must still emit a full sample.
        t.poll_once().await;
        let latest = t.latest().await;
        assert_eq!(latest[SLOT].socket_power_w, 299.0);
        assert_eq!(latest[SLOT].gfx_activity_pct, Some(100));
    }
}
