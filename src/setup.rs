use serde::Serialize;

use crate::driver::Gpu;

pub const OVERDRIVE_BIT: u32 = 0x4000;

#[derive(Debug, Clone, Serialize)]
pub struct GpuSetup {
    pub slot: String,
    pub overdrive_table: bool,
    pub pmfw_fan: bool,
    pub hwmon: bool,
    pub dpm_levels: bool,
    pub thermal_target: bool,
    pub power_profile: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SetupCheck {
    pub ready: bool,
    pub live_mask: Option<u32>,
    pub overdrive_bit: bool,
    pub gpus: Vec<GpuSetup>,
}

/// Live overdrive feature mask from the amdgpu module parameter.
pub fn read_live_mask() -> Option<u32> {
    std::fs::read_to_string("/sys/module/amdgpu/parameters/ppfeaturemask")
        .ok()
        .and_then(|s| {
            let t = s.trim();
            u32::from_str_radix(t.trim_start_matches("0x"), 16).ok()
        })
}

pub async fn run(gpus: &[Gpu], live_mask: Option<u32>) -> SetupCheck {
    let mut out = Vec::new();
    for g in gpus {
        let overdrive_table = g
            .root
            .read_all(&g.rel("pp_od_clk_voltage"))
            .await
            .ok()
            .map(|raw| crate::driver::od::parse_table(&raw).is_ok())
            .unwrap_or(false);
        let pmfw_fan = g.read_fan_curve().await.ok().flatten().is_some();
        let hwmon = g.root.exists(&g.h("power1_cap")).await;
        let dpm_levels = g
            .static_info()
            .await
            .map(|s| !s.dpm_sclk_levels.is_empty())
            .unwrap_or(false);
        let thermal_target =
            g.root.exists(&g.rel(crate::driver::THERMAL_TARGET_NODE)).await;
        let power_profile =
            g.root.exists(&g.rel(crate::driver::POWER_PROFILE_NODE)).await;
        out.push(GpuSetup {
            slot: g.slot.clone(),
            overdrive_table,
            pmfw_fan,
            hwmon,
            dpm_levels,
            thermal_target,
            power_profile,
        });
    }
    let overdrive_bit = live_mask.map(|m| m & OVERDRIVE_BIT != 0).unwrap_or(false);
    let ready = overdrive_bit
        && !out.is_empty()
        && out
            .iter()
            .all(|g| g.overdrive_table && g.pmfw_fan && g.hwmon);
    SetupCheck {
        ready,
        live_mask,
        overdrive_bit,
        gpus: out,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::driver::sysfs::{FsSysfs, Sysfs};
    use crate::driver::Gpu;
    use std::sync::Arc;

    async fn fixture_gpu() -> Gpu {
        let fs = FsSysfs {
            root: std::path::PathBuf::from("fixtures/sysfs"),
        };
        let uevent = fs.read_all("card1/device/uevent").await.unwrap();
        Gpu {
            root: Arc::new(fs),
            rel: "card1".into(),
            slot: Gpu::slot_from_uevent(&uevent).unwrap(),
            card_n: 1,
            hwmon: "hwmon5".into(),
        }
    }

    #[tokio::test]
    async fn setup_check_on_fixture_passes() {
        let g = fixture_gpu().await;
        let chk = run(&[g], Some(0xfff7ffff)).await;
        assert!(chk.overdrive_bit);
        assert!(chk.ready);
        assert_eq!(chk.gpus.len(), 1);
        let g0 = &chk.gpus[0];
        assert!(g0.overdrive_table);
        assert!(g0.pmfw_fan);
        assert!(g0.hwmon);
        assert!(g0.dpm_levels);
        // New capability flags are informational (not in the ready gate).
        // `g0` is a `GpuSetup` (plain `bool` fields, not JSON): the fixture
        // ships `pp_power_profile_mode` but no `pp_dpm_tjmax` node.
        assert!(g0.power_profile);
        assert!(!g0.thermal_target);
        assert_eq!(g0.slot, "1002:7550-148c:2435-0000:0f:00.0");
    }

    #[tokio::test]
    async fn setup_check_without_bit_not_ready() {
        let g = fixture_gpu().await;
        let chk = run(&[g], Some(0xfff7ffff & !OVERDRIVE_BIT)).await;
        assert!(!chk.overdrive_bit);
        assert!(!chk.ready);
    }

    #[tokio::test]
    async fn setup_check_empty_gpus_not_ready() {
        let chk = run(&[], Some(0xfff7ffff)).await;
        assert!(!chk.ready);
    }
}
