use serde::{Deserialize, Serialize};

// ---------- Telemetry ----------

/// One poll of one GPU. `slot` is the stable PCI slot id.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GpuSample {
    pub slot: String,
    pub ts_ms: i64,
    pub socket_power_w: f64,
    pub power_cap_w: Option<f64>,
    pub edge_c: Option<i32>,
    pub hotspot_c: Option<i32>,
    pub vram_c: Option<i32>,
    pub gfx_mhz: Option<i32>,
    pub mem_mhz: Option<i32>,
    pub gfx_activity_pct: Option<i32>,
    /// UMC memory-bandwidth utilization 0-100 from `device/mem_busy_percent`
    /// (`None` when the file is absent or unparseable).
    pub mem_activity_pct: Option<i32>,
    pub vram_used_mb: Option<i64>,
    pub vram_total_mb: Option<i64>,
    /// Active DPM level (the `*`-marked line of pp_dpm_sclk/mclk).
    pub dpm_sclk_mhz: Option<i32>,
    pub dpm_mclk_mhz: Option<i32>,
    /// Throttle bit names from `gpu_metrics` (best-effort; `None` when
    /// libdrm / the struct version is unavailable).
    pub throttle_bits: Option<Vec<String>>,
    pub fan_rpm: Option<i32>,
    /// Fan duty 0-100 (best-effort via `gpu_metrics`; R9700 hwmon `pwm1`
    /// presence unverified — the 9070 snapshot ships `pwm1`/`fan1_*`).
    pub fan_pwm_pct: Option<f64>,
    /// Core (GFX) rail voltage in mV from `gpu_metrics` MetricsInfo
    /// (`None` when unreadable).
    pub voltage_gfx_mv: Option<i32>,
    /// SOC rail voltage in mV from `gpu_metrics` MetricsInfo.
    pub voltage_soc_mv: Option<i32>,
    /// Memory rail voltage in mV from `gpu_metrics` MetricsInfo.
    pub voltage_mem_mv: Option<i32>,
    /// UMC (VRAM) ECC correctable count (cumulative since last reset).
    /// `None` when the RAS file is unreadable or unparseable.
    pub ecc_umc_ce: Option<u64>,
    /// UMC ECC uncorrectable count (data lost).
    pub ecc_umc_ue: Option<u64>,
    /// UMC ECC deferred (retired) count; `None` if the file has no `de` line.
    pub ecc_umc_deferred: Option<u64>,
    /// Negotiated PCIe link width in lanes from `gpu_metrics` MetricsInfo
    /// (`None` when unreadable).
    pub pcie_width: Option<u16>,
    /// Negotiated PCIe link speed in GT/s (blob raw unit is 100 MHz).
    pub pcie_speed_gt: Option<f64>,
}

/// Slow-changing per-GPU discovery, computed at enumeration/status time.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct GpuStatic {
    pub slot: String,
    pub card_n: u32,
    pub vbios: Option<String>,
    /// Slowdown thresholds from hwmon `temp{i}_crit`.
    pub slowdown_edge_c: Option<i32>,
    pub slowdown_hotspot_c: Option<i32>,
    pub slowdown_vram_c: Option<i32>,
    pub cap_min_w: Option<i32>,
    pub cap_max_w: Option<i32>,
    /// `power1_cap_default` (spec §7 telemetry table).
    pub cap_default_w: Option<i32>,
    pub dpm_sclk_levels: Vec<i32>,
    pub dpm_mclk_levels: Vec<i32>,
    /// `pp_od_clk_voltage` present (overdrive bit enabled).
    pub overdrive: bool,
    /// From `OD_RANGE` in `pp_od_clk_voltage`; `None` when overdrive is
    /// unavailable or the range line is absent (daemon refuses to guess).
    pub sclk_offset_range_mhz: Option<(i32, i32)>,
    pub mclk_range_mhz: Option<(i32, i32)>,
    pub voltage_offset_range_mv: Option<(i32, i32)>,
    /// `gpu_od/fan_ctrl/fan_curve` parseable (PMFW fan control available).
    pub pmfw_fan: bool,
    pub thermal_target: bool,
    pub power_profile: bool,
}

// ---------- Presets ----------

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct PresetSettings {
    pub power_cap_w: Option<f64>,
    /// RDNA4 exposes no absolute max sclk, only a table offset (spec §9).
    pub sclk_offset_mhz: Option<i32>,
    pub max_mclk_mhz: Option<i32>,
    /// Undervolt, negatives only (mV).
    pub voltage_offset_mv: Option<i32>,
}

impl PresetSettings {
    /// Number of distinct knobs set (drives the "changes N knobs" badge).
    pub fn knob_count(&self) -> u32 {
        u32::from(self.power_cap_w.is_some())
            + u32::from(self.sclk_offset_mhz.is_some())
            + u32::from(self.max_mclk_mhz.is_some())
            + u32::from(self.voltage_offset_mv.is_some())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Preset {
    pub id: Option<i64>,
    pub name: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub settings: PresetSettings,
    #[serde(default)]
    pub is_builtin: bool,
    pub created_at: Option<i64>,
    pub updated_at: Option<i64>,
}

impl Preset {
    #[allow(dead_code)]
    pub fn knob_count(&self) -> u32 {
        self.settings.knob_count()
    }
}

// ---------- Control ----------

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ApplyOutcome {
    pub field: String,
    /// "verified" | "applied" | "failed" | "skipped"
    pub status: String,
    pub detail: String,
}

// ---------- Events ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: i64,
    pub ts: i64,
    pub kind: String,
    pub detail: serde_json::Value,
}

/// One drift episode (spec §5.3). `detected`/`intended` are the four knob
/// values as JSON objects; `intended` is `None` when the slot was in stock
/// state (nothing to restore).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DriftEntry {
    pub id: i64,
    pub ts: i64,
    pub slot: String,
    pub detected: serde_json::Value,
    pub intended: Option<serde_json::Value>,
    pub previous_source: String,
    /// "pending" | "reapplied" | "reapply_failed" | "cleared" |
    /// "logged_only" | "adopted"
    pub outcome: String,
    pub outcome_detail: Option<serde_json::Value>,
}

pub fn is_pci_slot_id(s: &str) -> bool {
    let parts: Vec<&str> = s.splitn(3, '-').collect();
    if parts.len() < 3 {
        return false;
    }
    parts[0].len() == 9
        && parts[0].as_bytes()[4] == b':'
        && parts[0][..4].chars().all(|c| c.is_ascii_hexdigit())
        && parts[0][5..].chars().all(|c| c.is_ascii_hexdigit())
        && parts[1].len() == 9
        && parts[1].as_bytes()[4] == b':'
        && parts[1][..4].chars().all(|c| c.is_ascii_hexdigit())
        && parts[1][5..].chars().all(|c| c.is_ascii_hexdigit())
}

/// v2 settings shape: a single `PresetSettings` object (keys are knob
/// names). v1 was `BTreeMap<slot, PresetSettings>` (keys are PCI slot ids).
pub fn is_v2_settings_shape(v: &serde_json::Value) -> bool {
    let obj = match v.as_object() {
        Some(o) => o,
        None => return false,
    };
    !obj.keys().any(|k| is_pci_slot_id(k))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn knob_count_counts_set_fields() {
        assert_eq!(PresetSettings::default().knob_count(), 0);
        let s = PresetSettings {
            power_cap_w: Some(235.0),
            sclk_offset_mhz: Some(100),
            max_mclk_mhz: Some(1258),
            voltage_offset_mv: Some(-50),
        };
        assert_eq!(s.knob_count(), 4);
    }

    #[test]
    fn is_pci_slot_id_valid_and_invalid() {
        assert!(is_pci_slot_id("1002:75b4-1458:242f-0000:0a:00.0"));
        assert!(is_pci_slot_id("1002:7550-148c:2435-0000:0f:00.0"));
        assert!(!is_pci_slot_id("power_cap_w"));
        assert!(!is_pci_slot_id("fan"));
        assert!(!is_pci_slot_id(""));
        assert!(!is_pci_slot_id("1002:75b4"));
    }

    #[test]
    fn is_v2_settings_shape() {
        let v2: serde_json::Value =
            serde_json::from_str(r#"{"power_cap_w":250.0,"fan":{}}"#).unwrap();
        assert!(super::is_v2_settings_shape(&v2));
        let empty: serde_json::Value = serde_json::from_str("{}").unwrap();
        assert!(super::is_v2_settings_shape(&empty));
        let v1: serde_json::Value =
            serde_json::from_str(r#"{"1002:75b4-1458:242f-0000:0a:00.0":{"power_cap_w":250.0}}"#)
                .unwrap();
        assert!(!super::is_v2_settings_shape(&v1));
    }

    #[test]
    fn preset_knob_count_delegates() {
        let p = Preset {
            id: None,
            name: "n".into(),
            notes: String::new(),
            settings: PresetSettings {
                power_cap_w: Some(250.0),
                sclk_offset_mhz: Some(50),
                ..Default::default()
            },
            is_builtin: false,
            created_at: None,
            updated_at: None,
        };
        assert_eq!(p.knob_count(), 2);
    }

    #[test]
    fn preset_settings_sparse_roundtrip() {
        // Spec §9 example: unset fields stay absent (sparse overlay).
        let s: PresetSettings = serde_json::from_str(
            r#"{"power_cap_w": 235.0, "sclk_offset_mhz": 100, "voltage_offset_mv": -50}"#,
        )
        .unwrap();
        assert_eq!(s.power_cap_w, Some(235.0));
        assert_eq!(s.max_mclk_mhz, None);
    }

    #[test]
    fn sample_shape() {
        let s = GpuSample {
            slot: "1002:75b4-1458:242f-0000:0b:00.0".into(),
            ts_ms: 1,
            socket_power_w: 299.0,
            power_cap_w: Some(300.0),
            edge_c: Some(78),
            hotspot_c: Some(95),
            vram_c: Some(85),
            gfx_mhz: Some(3110),
            mem_mhz: Some(1258),
            gfx_activity_pct: Some(100),
            mem_activity_pct: Some(69),
            vram_used_mb: Some(8192),
            vram_total_mb: Some(32768),
            dpm_sclk_mhz: Some(3110),
            dpm_mclk_mhz: Some(1258),
            throttle_bits: Some(vec!["THROTTLE_THERMAL".into()]),
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
        };
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["slot"], s.slot);
        let e = Event {
            id: 1,
            ts: 2,
            kind: "apply".into(),
            detail: json!({"a": 1}),
        };
        assert_eq!(serde_json::to_value(&e).unwrap()["kind"], "apply");
    }
}
