pub mod ecc;
pub mod fake;
pub mod fixture;
pub mod fan;
pub mod od;
pub mod power;
pub mod read;
pub mod sysfs;

use std::sync::Arc;

use self::sysfs::{FsSysfs, Sysfs};
use crate::types::GpuStatic;

/// Phase 2a knob node names (spec §3). `POWER_PROFILE_NODE` is confirmed on
/// the day-1 gate. `THERMAL_TARGET_NODE` is an unverified assumption — if a
/// different name turns up, change it here only. Both are used by capability
/// detection in `static_info`.
pub const POWER_PROFILE_NODE: &str = "pp_power_profile_mode";
pub const THERMAL_TARGET_NODE: &str = "pp_dpm_tjmax";

/// One enumerated GPU. `rel` is the path component under the sysfs root,
/// e.g. `"card1"`. All writes go through `Sysfs`, never raw paths.
#[derive(Clone)]
pub struct Gpu {
    pub root: Arc<dyn Sysfs>,
    pub rel: String,
    /// Stable PCI slot id, e.g. "1002:75b4-1458:242f-0000:0b:00.0".
    pub slot: String,
    pub card_n: u32,
    /// Kernel-picked hwmon index, e.g. `"hwmon5"`. Discovered at
    /// enumeration (list_dir of `device/hwmon`); hardcoded in test fakes.
    pub hwmon: String,
}

impl Gpu {
    pub fn rel(&self, part: &str) -> String {
        format!("{}/device/{}", self.rel, part)
    }
    /// hwmon path helper: `{rel}/device/hwmon/{hwmon}/{part}`.
    pub fn h(&self, part: &str) -> String {
        self.rel(&format!("hwmon/{}/{}", self.hwmon, part))
    }
}

impl Gpu {
    /// `"VEN:DEV-SUBSYSVEN:SUBSYSDEV-BDF"`, lowercase.
    pub fn slot_from_uevent(raw: &str) -> anyhow::Result<String> {
        let mut id: Option<&str> = None;
        let mut subsys: Option<&str> = None;
        let mut slot: Option<&str> = None;
        for line in raw.lines() {
            if let Some((k, v)) = line.split_once('=') {
                match k {
                    "PCI_ID" => id = Some(v.trim()),
                    "PCI_SUBSYS_ID" => subsys = Some(v.trim()),
                    "PCI_SLOT_NAME" => slot = Some(v.trim()),
                    _ => {}
                }
            }
        }
        let (id, subsys, slot) = (
            id.ok_or_else(|| anyhow::anyhow!("uevent missing PCI_ID"))?,
            subsys.ok_or_else(|| anyhow::anyhow!("uevent missing PCI_SUBSYS_ID"))?,
            slot.ok_or_else(|| anyhow::anyhow!("uevent missing PCI_SLOT_NAME"))?,
        );
        Ok(format!("{id}-{subsys}-{slot}").to_lowercase())
    }

    async fn temp_crit_by_label(&self, want: &str) -> anyhow::Result<Option<i32>> {
        for i in 1..=10 {
            let l = match self.root.read(&self.h(&format!("temp{i}_label"))).await {
                Ok(l) => l,
                Err(_) => break,
            };
            if l.trim().eq_ignore_ascii_case(want) {
                let v = self.root.read(&self.h(&format!("temp{i}_crit"))).await?;
                let mv: i32 = v.parse().map_err(|_| anyhow::anyhow!("crit parse"))?;
                return Ok(Some(mv / 1000));
            }
        }
        Ok(None)
    }

    async fn dpm_levels(&self, rel: &str) -> Vec<i32> {
        match self.root.read_all(&self.rel(rel)).await {
            Ok(raw) => self::read::parse_dpm(&raw)
                .map(|(lv, _)| lv)
                .unwrap_or_default(),
            Err(_) => Vec::new(),
        }
    }

    pub async fn static_info(&self) -> anyhow::Result<GpuStatic> {
        let cap_min = self
            .root
            .read(&self.h("power1_cap_min"))
            .await
            .ok()
            .and_then(|s| s.parse::<i64>().ok())
            .map(|u| (u / 1_000_000) as i32);
        let cap_max = self
            .root
            .read(&self.h("power1_cap_max"))
            .await
            .ok()
            .and_then(|s| s.parse::<i64>().ok())
            .map(|u| (u / 1_000_000) as i32);
        let cap_default = self
            .root
            .read(&self.h("power1_cap_default"))
            .await
            .ok()
            .and_then(|s| s.parse::<i64>().ok())
            .map(|u| (u / 1_000_000) as i32);

        // Hoist the OD-table read; overdrive = file present AND parseable.
        // Keep the parsed table so its `OD_RANGE` bounds reach the UI.
        let od = self
            .root
            .read_all(&self.rel("pp_od_clk_voltage"))
            .await
            .ok()
            .and_then(|raw| self::od::parse_table(&raw).ok());
        let overdrive = od.is_some();
        let pmfw_fan = self.read_fan_curve().await?.is_some();
        let power_profile = self.root.exists(&self.rel(POWER_PROFILE_NODE)).await;
        let thermal_target = self.root.exists(&self.rel(THERMAL_TARGET_NODE)).await;

        // Hotspot label is "junction" on this kernel; fall back to "hotspot".
        let hotspot = match self.temp_crit_by_label("junction").await {
            Ok(Some(v)) => Some(v),
            _ => self.temp_crit_by_label("hotspot").await?,
        };

        Ok(GpuStatic {
            slot: self.slot.clone(),
            card_n: self.card_n,
            vbios: None, // not exposed on this driver; reserved
            slowdown_edge_c: self.temp_crit_by_label("edge").await?,
            slowdown_hotspot_c: hotspot,
            slowdown_vram_c: self.temp_crit_by_label("mem").await?,
            cap_min_w: cap_min,
            cap_max_w: cap_max,
            cap_default_w: cap_default,
            dpm_sclk_levels: self.dpm_levels("pp_dpm_sclk").await,
            dpm_mclk_levels: self.dpm_levels("pp_dpm_mclk").await,
            overdrive,
            sclk_offset_range_mhz: od.as_ref().and_then(|t| t.ranges.sclk_offset),
            mclk_range_mhz: od.as_ref().and_then(|t| t.ranges.mclk),
            voltage_offset_range_mv: od.as_ref().and_then(|t| t.ranges.vddgfx),
            pmfw_fan,
            thermal_target,
            power_profile,
        })
    }
}

/// Scan the production sysfs root for `cardN` dirs and build a `Gpu` each.
/// Retries a card whose uevent is not yet readable (driver still loading),
/// up to `attempts` times, `delay` apart.
pub async fn enum_gpus(
    root: &FsSysfs,
    attempts: u32,
    delay: std::time::Duration,
) -> anyhow::Result<Vec<Gpu>> {
    let entries = std::fs::read_dir(&root.root)
        .map_err(|e| anyhow::anyhow!("read sysfs root {}: {e}", root.root.display()))?;
    let mut cards: Vec<String> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| {
            let b = n.strip_prefix("card").unwrap_or(n.as_str());
            n.starts_with("card") && b.chars().all(|c| c.is_ascii_digit())
        })
        .collect();
    cards.sort();

    let mut out = Vec::new();
    for name in cards {
        let uevent_rel = format!("{name}/device/uevent");
        let mut raw: Option<String> = None;
        for _ in 0..attempts {
            if let Ok(r) = root.read_all(&uevent_rel).await {
                raw = Some(r);
                break;
            }
            tokio::time::sleep(delay).await;
        }
        let raw = match raw {
            Some(r) => r,
            None => {
                tracing::warn!("{name}: uevent unreadable after {attempts} tries, skipping");
                continue;
            }
        };
        // A broken card must not kill enumeration of the working ones
        // (spec §11: daemon comes up with N−1 GPUs) — warn + continue.
        let slot = match Gpu::slot_from_uevent(&raw) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("{name}: unparseable uevent ({e}), skipping");
                continue;
            }
        };
        let card_n = name
            .strip_prefix("card")
            .and_then(|n| n.parse::<u32>().ok())
            .unwrap_or(0);
        // Discover the kernel-picked hwmon index (e.g. "hwmon5").
        let hwmon = match root
            .list_dir(&format!("{name}/device/hwmon"))
            .await
            .ok()
            .and_then(|mut d| d.pop())
        {
            Some(h) => h,
            None => {
                tracing::warn!("{name}: no hwmon dir, skipping");
                continue;
            }
        };
        out.push(Gpu {
            root: std::sync::Arc::new(FsSysfs {
                root: root.root.clone(),
            }),
            rel: name,
            slot,
            card_n,
            hwmon,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::driver::sysfs::MemSysfs;
    use std::sync::Arc;

    const UEVENT: &str = "PCI_ID=1002:75B4\nPCI_SUBSYS_ID=1458:242F\nPCI_SLOT_NAME=0000:0b:00.0\n";

    #[test]
    fn slot_from_uevent_lowercases_and_orders() {
        let s = Gpu::slot_from_uevent(UEVENT).unwrap();
        assert_eq!(s, "1002:75b4-1458:242f-0000:0b:00.0");
    }

    #[test]
    fn slot_from_uevent_requires_all_fields() {
        assert!(Gpu::slot_from_uevent("PCI_ID=1002:75B4\n").is_err());
    }

    async fn gpu_with(files: &[(&str, &str)]) -> Gpu {
        let s = MemSysfs::new();
        for (k, v) in files {
            s.put(k, v).await;
        }
        Gpu {
            root: Arc::new(s),
            rel: "card0".into(),
            slot: "1002:75b4-1458:242f-0000:0b:00.0".into(),
            card_n: 0,
            hwmon: "hwmon0".into(),
        }
    }

    #[tokio::test]
    async fn static_info_reads_thresholds_and_caps() {
        let g = gpu_with(&[
            ("card0/device/hwmon/hwmon0/temp1_label", "edge"),
            ("card0/device/hwmon/hwmon0/temp1_crit", "112000"),
            ("card0/device/hwmon/hwmon0/temp2_label", "junction"),
            ("card0/device/hwmon/hwmon0/temp2_crit", "116000"),
            ("card0/device/hwmon/hwmon0/temp3_label", "mem"),
            ("card0/device/hwmon/hwmon0/temp3_crit", "105000"),
            ("card0/device/hwmon/hwmon0/power1_cap_min", "120000000"),
            ("card0/device/hwmon/hwmon0/power1_cap_max", "300000000"),
            ("card0/device/pp_dpm_sclk", "S: 19Mhz *\n0: 615Mhz\n1: 3110Mhz"),
            ("card0/device/pp_dpm_mclk", "0: 96Mhz\n1: 1258Mhz *"),
            ("card0/device/pp_od_clk_voltage", "OD_SCLK_OFFSET:\n0Mhz\nOD_MCLK:\n0: 97Mhz\n1: 1258MHz\nOD_VDDGFX_OFFSET:\n0mV\nSCLK_OFFSET:    -500Mhz       1000Mhz\nMCLK:      97Mhz       1500Mhz\nVDDGFX_OFFSET:    -200mv          0mv"),
            ("card0/device/gpu_od/fan_ctrl/fan_curve", "OD_FAN_CURVE:\n0: 25C 30%\nOD_RANGE:\nFAN_CURVE(hotspot temp): 25C 100C\nFAN_CURVE(fan speed): 30% 100%"),
        ])
        .await;
        let st = g.static_info().await.unwrap();
        assert_eq!(st.slowdown_edge_c, Some(112));
        assert_eq!(st.slowdown_hotspot_c, Some(116));
        assert_eq!(st.slowdown_vram_c, Some(105));
        assert_eq!(st.cap_min_w, Some(120));
        assert_eq!(st.cap_max_w, Some(300));
        // numbered lines only — the `S:` soft-min line is not a level
        assert_eq!(st.dpm_sclk_levels, vec![615, 3110]);
        assert_eq!(st.dpm_mclk_levels, vec![96, 1258]);
        assert!(st.overdrive);
        assert!(st.pmfw_fan);
        assert_eq!(st.sclk_offset_range_mhz, Some((-500, 1000)));
        assert_eq!(st.mclk_range_mhz, Some((97, 1500)));
        assert_eq!(st.voltage_offset_range_mv, Some((-200, 0)));
    }

    #[tokio::test]
    async fn static_info_missing_od_marks_no_overdrive() {
        let g = gpu_with(&[
            ("card0/device/hwmon/hwmon0/power1_cap_min", "120000000"),
            ("card0/device/hwmon/hwmon0/power1_cap_max", "300000000"),
            ("card0/device/pp_dpm_sclk", "S: 19Mhz *\n0: 615Mhz"),
            ("card0/device/pp_dpm_mclk", "0: 96Mhz *"),
        ])
        .await;
        let st = g.static_info().await.unwrap();
        assert!(!st.overdrive);
        assert!(!st.pmfw_fan);
        assert_eq!(st.sclk_offset_range_mhz, None);
        assert_eq!(st.mclk_range_mhz, None);
        assert_eq!(st.voltage_offset_range_mv, None);
        assert_eq!(st.dpm_sclk_levels, vec![615]);
    }

    #[tokio::test]
    async fn enum_gpus_finds_cards_and_skips_junk() {
        let dir = tempfile::tempdir().unwrap();
        let root_path = dir.path().to_path_buf();
        // One real card; a non-card dir; a `card` prefix without digits.
        std::fs::create_dir_all(root_path.join("card0/device/hwmon/hwmon5")).unwrap();
        std::fs::write(root_path.join("card0/device/uevent"), UEVENT).unwrap();
        std::fs::create_dir_all(root_path.join("apex")).unwrap();
        std::fs::create_dir_all(root_path.join("cardx/device")).unwrap();
        let root = FsSysfs { root: root_path };
        let gpus = enum_gpus(&root, 1, std::time::Duration::from_millis(1))
            .await
            .unwrap();
        assert_eq!(gpus.len(), 1);
        assert_eq!(gpus[0].rel, "card0");
        assert_eq!(gpus[0].slot, "1002:75b4-1458:242f-0000:0b:00.0");
        assert_eq!(gpus[0].card_n, 0);
        assert_eq!(gpus[0].hwmon, "hwmon5");
    }
}
