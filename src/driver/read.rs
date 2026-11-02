use super::Gpu;

/// Cheap fast-path fields, read every `--record-s` (spec §3.3).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FastFields {
    pub socket_power_w: Option<f64>,
    pub edge_c: Option<i32>,
    pub hotspot_c: Option<i32>,
    pub vram_c: Option<i32>,
    pub gfx_mhz: Option<i32>,
    pub mem_mhz: Option<i32>,
    pub gfx_activity_pct: Option<i32>,
    pub mem_activity_pct: Option<i32>,
    pub vram_used_mb: Option<i64>,
    pub dpm_sclk_mhz: Option<i32>,
    /// The `dpm_mclk.or(mem)` fallback is already applied.
    pub dpm_mclk_mhz: Option<i32>,
}

/// Slow-path setpoint/constant fields, read once per period (spec §3.3).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SlowFields {
    pub power_cap_w: Option<f64>,
    pub vram_total_mb: Option<i64>,
}

use std::collections::HashMap;
use std::sync::{Mutex as StdMutex, OnceLock};

/// Process-global temp label→index cache. hwmon temp labels are static per
/// card, so scanning `temp1..temp10_label` once is safe (spec §3.3). Keyed by
/// `(rel, hwmon, label)`. Only positive hits are cached: a label not exposed
/// yet (driver still probing) is re-scanned on the next read.
static TEMP_LABEL_INDEX: OnceLock<StdMutex<HashMap<String, u32>>> = OnceLock::new();

fn temp_label_index() -> &'static StdMutex<HashMap<String, u32>> {
    TEMP_LABEL_INDEX.get_or_init(|| StdMutex::new(HashMap::new()))
}

/// Parse a `pp_dpm_sclk`/`pp_dpm_mclk` table. Real format (multi-line,
/// one `IDX: VALUEMhz` per line; trailing spaces common; active level
/// marked with a trailing ` *`; the sclk table may carry a non-numeric
/// `S:` soft-min line). Driver-bug workarounds (spec §7 research note):
/// a duplicate ` *` marker takes the **first** occurrence; after sleep
/// the table can be 1-based — irrelevant here because we return values,
/// not indices.
///
/// Returns `(levels, active)`: `levels` = numbered lines only, in file
/// order; `active` = value of the `*`-marked line, or `None` if unmarked.
pub fn parse_dpm(raw: &str) -> anyhow::Result<(Vec<i32>, Option<i32>)> {
    let mut levels: Vec<i32> = Vec::new();
    let mut active: Option<i32> = None;
    for line in raw.lines() {
        let line = line.trim();
        let (idx_part, val_raw) = match line.split_once(':') {
            Some(p) => p,
            None => continue,
        };
        let val_trim = val_raw.trim();
        let starred = val_trim.ends_with('*');
        let v: Option<i32> = val_trim
            .trim_end_matches('*')
            .trim()
            .strip_suffix("Mhz")
            .or_else(|| val_trim.trim_end_matches('*').trim().strip_suffix("MHz"))
            .and_then(|v| v.trim().parse::<i32>().ok());
        let Some(v) = v else { continue };
        if starred && active.is_none() {
            active = Some(v);
        }
        if !idx_part.trim().is_empty() && idx_part.trim().chars().all(|c| c.is_ascii_digit()) {
            levels.push(v);
        }
    }
    Ok((levels, active))
}

/// Best-effort extras from the `gpu_metrics` (MetricsInfo) read: throttle
/// bits, fan values, and rail voltages in mV. All fields `None` when the
/// read fails.
#[derive(Debug, Clone, Default)]
pub struct GpuMetricsExtra {
    pub throttle: Option<Vec<String>>,
    pub fan_rpm: Option<i32>,
    pub fan_pwm: Option<f64>,
    pub voltage_gfx_mv: Option<i32>,
    pub voltage_soc_mv: Option<i32>,
    pub voltage_mem_mv: Option<i32>,
    /// Negotiated PCIe link width in lanes (best-effort via gpu_metrics).
    pub pcie_width: Option<u16>,
    /// Negotiated PCIe link speed in GT/s (blob raw unit is 100 MHz).
    pub pcie_speed_gt: Option<f64>,
}

/// Best-effort gpu_metrics read via libdrm (read-only; real sysfs path).
///
/// libdrm_amdgpu_sys 0.8.16 API (verified against the crate source):
/// everything lives under the `AMDGPU` module; `MetricsInfo` is a trait
/// implemented by `GpuMetrics` itself (there is no `MetricsInfo::new`);
/// the accessors return `Option`, not `Result`; and
/// `get_from_sysfs_path` takes the *directory containing* `gpu_metrics`
/// (it joins `gpu_metrics` onto the path itself) — hence `/device`.
pub fn gpu_metrics_for(root: &str, rel: &str) -> GpuMetricsExtra {
    use libdrm_amdgpu_sys::AMDGPU::MetricsInfo as _;
    let path = format!("{root}/{rel}/device");
    let Ok(metrics) = libdrm_amdgpu_sys::AMDGPU::GpuMetrics::get_from_sysfs_path(&path) else {
        return GpuMetricsExtra::default();
    };
    let throttle = metrics.get_throttle_status_info().map(|t| {
        t.get_all_throttler()
            .into_iter()
            .map(|b| b.to_string())
            .collect()
    });
    let fan_rpm = metrics.get_current_fan_speed().map(|r| r as i32);
    let fan_pwm = metrics.get_fan_pwm().map(|p| {
        let p = p as f64;
        if p <= 100.0 {
            p
        } else {
            p / 255.0 * 100.0
        }
    });
    let voltage_gfx_mv = metrics.get_voltage_gfx().map(|v| v as i32);
    let voltage_soc_mv = metrics.get_voltage_soc().map(|v| v as i32);
    let voltage_mem_mv = metrics.get_voltage_mem().map(|v| v as i32);
    let pcie_width = metrics.get_pcie_link_width();
    let pcie_speed_gt = metrics.get_pcie_link_speed().map(|s| s as f64 / 10.0);
    GpuMetricsExtra {
        throttle,
        fan_rpm,
        fan_pwm,
        voltage_gfx_mv,
        voltage_soc_mv,
        voltage_mem_mv,
        pcie_width,
        pcie_speed_gt,
    }
}

/// Best-effort RAS `umc_err_count` read for UMC ECC counters (spec §5.2).
/// Reads the real `{root}/{rel}/device/ras/umc_err_count` text file and
/// delegates to the pure `parse_umc_err_count`. Returns `None` when the
/// file is absent, unreadable, or unparseable.
pub fn ecc_for(root: &str, rel: &str) -> Option<crate::driver::ecc::EccCounters> {
    let path = format!("{root}/{rel}/device/ras/umc_err_count");
    let text = std::fs::read_to_string(&path).ok()?;
    crate::driver::ecc::parse_umc_err_count(&text)
}

impl Gpu {
    async fn scan_temp_index(&self, want: &str) -> Option<u32> {
        for i in 1..=10u32 {
            let l = match self.root.read(&self.h(&format!("temp{i}_label"))).await {
                Ok(l) => l,
                Err(_) => break,
            };
            if l.trim().eq_ignore_ascii_case(want) {
                return Some(i);
            }
        }
        None
    }

    async fn temp_by_label(&self, want: &str) -> anyhow::Result<Option<i32>> {
        let key = format!("{}/{}/{}", self.rel, self.hwmon, want);
        // Bind the lookup in a `let` so the MutexGuard drops at end of
        // statement — holding it across the match would self-deadlock on
        // the insert below (std Mutex is non-reentrant).
        let cached = temp_label_index().lock().unwrap().get(&key).copied();
        let idx: Option<u32> = match cached {
            Some(i) => Some(i),
            None => {
                let found = self.scan_temp_index(want).await;
                // Cache positive hits only (see the cache docs above).
                if let Some(i) = found {
                    temp_label_index().lock().unwrap().insert(key, i);
                }
                found
            }
        };
        // Label not found: same `Ok(None)` semantics as the old scan loop.
        let Some(idx) = idx else {
            return Ok(None);
        };
        // A cached index can point at a fixture/root that has no `tempN_input`
        // (e.g. tests sharing the `card0/hwmon0` key without temp files).
        // Deliberate deviation from the old scan loop, which `Err`'d here:
        // `Ok(None)` is required for the shared-key fixtures, and no test
        // depends on the old `Err` path. Only a present-but-unparseable value
        // is an error.
        let Ok(v) = self.root.read(&self.h(&format!("temp{idx}_input"))).await else {
            return Ok(None);
        };
        let mv: i32 = v.parse().map_err(|_| anyhow::anyhow!("temp parse"))?;
        Ok(Some(mv / 1000))
    }

    async fn opt_read(&self, rel: &str) -> Option<String> {
        self.root.read(rel).await.ok()
    }

    pub async fn read_fast_fields(&self) -> anyhow::Result<FastFields> {
        let h = |p: &str| self.h(p);

        let socket = self
            .opt_read(&h("power1_average"))
            .await
            .and_then(|s| s.parse::<i64>().ok())
            .map(|u| u as f64 / 1e6);
        let edge = self.temp_by_label("edge").await?;
        // hwmon label for the hotspot is "junction" on this kernel; fall back.
        let hotspot = match self.temp_by_label("junction").await {
            Ok(Some(v)) => Some(v),
            _ => self.temp_by_label("hotspot").await?,
        };
        let vram = self.temp_by_label("mem").await?;
        let gfx = self
            .opt_read(&h("freq1_input"))
            .await
            .and_then(|s| s.parse::<i64>().ok())
            .map(|z| (z / 1_000_000) as i32);
        let mem = self
            .opt_read(&h("freq2_input"))
            .await
            .and_then(|s| s.parse::<i64>().ok())
            .map(|z| (z / 1_000_000) as i32);
        // gpu_busy_percent lives under device/, not hwmon (spec §7).
        let busy = self
            .opt_read(&self.rel("gpu_busy_percent"))
            .await
            .and_then(|s| s.parse::<i32>().ok());
        let mem_busy = self
            .opt_read(&self.rel("mem_busy_percent"))
            .await
            .and_then(|s| s.parse::<i32>().ok());
        // Flat `device/mem_info_vram_*` files (no `mem_info/` subdirectory).
        let vu = self
            .opt_read(&self.rel("mem_info_vram_used"))
            .await
            .and_then(|s| s.parse::<i64>().ok())
            .map(|b| b / (1024 * 1024));
        let dpm_sclk = self
            .root
            .read_all(&self.rel("pp_dpm_sclk"))
            .await
            .ok()
            .and_then(|r| parse_dpm(&r).ok())
            .and_then(|(_, a)| a);
        // The mclk DPM table is fixed at the hardware levels; when the clock runs
        // above the top level the file has no `*` marker. Fall back to the hwmon
        // memory frequency — per sub-sample, before reduction (spec §3.3).
        let dpm_mclk = self
            .root
            .read_all(&self.rel("pp_dpm_mclk"))
            .await
            .ok()
            .and_then(|r| parse_dpm(&r).ok())
            .and_then(|(_, a)| a)
            .or(mem);

        Ok(FastFields {
            socket_power_w: socket,
            edge_c: edge,
            hotspot_c: hotspot,
            vram_c: vram,
            gfx_mhz: gfx,
            mem_mhz: mem,
            gfx_activity_pct: busy,
            mem_activity_pct: mem_busy,
            vram_used_mb: vu,
            dpm_sclk_mhz: dpm_sclk,
            dpm_mclk_mhz: dpm_mclk,
        })
    }

    pub async fn read_slow_fields(&self) -> SlowFields {
        let cap = self
            .opt_read(&self.h("power1_cap"))
            .await
            .and_then(|s| s.parse::<i64>().ok())
            .map(|u| u as f64 / 1e6);
        let vt = self
            .opt_read(&self.rel("mem_info_vram_total"))
            .await
            .and_then(|s| s.parse::<i64>().ok())
            .map(|b| b / (1024 * 1024));
        SlowFields {
            power_cap_w: cap,
            vram_total_mb: vt,
        }
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::driver::sysfs::MemSysfs;
    use std::sync::Arc;

    #[test]
    fn ecc_for_reads_ras_file_and_none_when_absent() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_string_lossy().to_string();
        // Absent file → None (best-effort, never errors).
        assert!(ecc_for(&root, "card0").is_none());
        // A RAS file on disk → the counters.
        std::fs::create_dir_all(dir.path().join("card0/device/ras")).unwrap();
        std::fs::write(
            dir.path().join("card0/device/ras/umc_err_count"),
            "ue: 9\nce: 7\nde: 1\n",
        )
        .unwrap();
        let c = ecc_for(&root, "card0").unwrap();
        assert_eq!((c.ce, c.ue, c.deferred), (7, 9, Some(1)));
    }

    #[test]
    fn parse_dpm_multiline_star_on_s_line() {
        let (lv, act) = parse_dpm("S: 19Mhz *\n0: 615Mhz\n1: 800Mhz").unwrap();
        assert_eq!(lv, vec![615, 800]);
        assert_eq!(act, Some(19));
    }

    #[test]
    fn parse_dpm_star_on_level_line() {
        let (lv, act) = parse_dpm("0: 96Mhz \n5: 1258Mhz *").unwrap();
        assert_eq!(lv, vec![96, 1258]);
        assert_eq!(act, Some(1258));
    }

    #[test]
    fn parse_dpm_duplicate_star_takes_first() {
        let (_, act) = parse_dpm("0: 96Mhz *\n5: 1258Mhz *").unwrap();
        assert_eq!(act, Some(96));
    }

    #[test]
    fn parse_dpm_no_star_is_none() {
        let (lv, act) = parse_dpm("0: 96Mhz\n1: 1258Mhz").unwrap();
        assert_eq!(lv, vec![96, 1258]);
        assert_eq!(act, None);
    }

    async fn sample_gpu() -> Gpu {
        let s = MemSysfs::new();
        let h = "card0/device/hwmon/hwmon0";
        s.put(&format!("{h}/power1_average"), "299000000").await;
        s.put(&format!("{h}/power1_cap"), "300000000").await;
        s.put(&format!("{h}/temp1_label"), "edge").await;
        s.put(&format!("{h}/temp1_input"), "78100").await;
        s.put(&format!("{h}/temp2_label"), "junction").await;
        s.put(&format!("{h}/temp2_input"), "95000").await;
        s.put(&format!("{h}/temp3_label"), "mem").await;
        s.put(&format!("{h}/temp3_input"), "85000").await;
        s.put(&format!("{h}/freq1_input"), "3110000000").await;
        s.put(&format!("{h}/freq2_input"), "1258000000").await;
        s.put("card0/device/gpu_busy_percent", "100").await;
        s.put("card0/device/mem_busy_percent", "69").await;
        s.put("card0/device/mem_info_vram_used", "8589934592").await;
        s.put("card0/device/mem_info_vram_total", "34359738368")
            .await;
        s.put(
            "card0/device/pp_dpm_sclk",
            "S: 19Mhz *\n0: 615Mhz\n1: 3110Mhz",
        )
        .await;
        s.put("card0/device/pp_dpm_mclk", "0: 96Mhz\n1: 1258Mhz *")
            .await;
        Gpu {
            root: Arc::new(s),
            rel: "card0".into(),
            slot: "1002:75b4-1458:242f-0000:0b:00.0".into(),
            card_n: 0,
            hwmon: "hwmon0".into(),
        }
    }

    #[tokio::test]
    async fn read_fast_fields_populates_fast_only() {
        let g = sample_gpu().await;
        let f = g.read_fast_fields().await.unwrap();
        assert_eq!(f.socket_power_w, Some(299.0));
        assert_eq!(f.edge_c, Some(78));
        assert_eq!(f.hotspot_c, Some(95));
        assert_eq!(f.vram_c, Some(85));
        assert_eq!(f.gfx_mhz, Some(3110));
        assert_eq!(f.mem_mhz, Some(1258));
        assert_eq!(f.gfx_activity_pct, Some(100));
        assert_eq!(f.mem_activity_pct, Some(69));
        assert_eq!(f.vram_used_mb, Some(8192));
        assert_eq!(f.dpm_sclk_mhz, Some(19));
        assert_eq!(f.dpm_mclk_mhz, Some(1258));
    }

    #[tokio::test]
    async fn read_slow_fields_populates_setpoints() {
        let g = sample_gpu().await;
        let s = g.read_slow_fields().await;
        assert_eq!(s.power_cap_w, Some(300.0));
        assert_eq!(s.vram_total_mb, Some(32768));
    }

    #[tokio::test]
    async fn read_fast_fields_none_when_absent() {
        // Distinct `rel` so the global temp-label cache doesn't see card0's labels.
        let g = Gpu {
            root: Arc::new(MemSysfs::new()),
            rel: "card9".into(),
            slot: "x".into(),
            card_n: 9,
            hwmon: "hwmon0".into(),
        };
        let f = g.read_fast_fields().await.unwrap();
        assert_eq!(f, FastFields::default());
    }

    #[tokio::test]
    async fn fast_fields_mclk_falls_back_to_hwmon_freq() {
        // Distinct `rel` (card9): this fixture has no temp labels, so sharing
        // card0's key with the label-layout tests would pollute the global
        // temp-label cache (Global Constraints).
        let s = MemSysfs::new();
        let h = "card9/device/hwmon/hwmon0";
        s.put(&format!("{h}/freq2_input"), "1400000000").await;
        // No `*` marker: mclk above the top listed level.
        s.put("card9/device/pp_dpm_mclk", "0: 96Mhz\n1: 1258Mhz").await;
        let g = Gpu {
            root: Arc::new(s),
            rel: "card9".into(),
            slot: "1002:75b4-1458:242f-0000:0b:00.0".into(),
            card_n: 9,
            hwmon: "hwmon0".into(),
        };
        let f = g.read_fast_fields().await.unwrap();
        assert_eq!(f.mem_mhz, Some(1400));
        assert_eq!(f.dpm_mclk_mhz, Some(1400)); // per-sub-sample fallback
    }
}
