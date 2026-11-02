use serde::Deserialize;

use crate::types::PresetSettings;

/// One entry of the embedded `data/builtin-presets.toml`.
#[derive(Debug, Clone, Deserialize)]
pub struct BuiltinPreset {
    pub name: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub settings: PresetSettings,
}

#[derive(Debug, Deserialize)]
struct File {
    #[serde(default = "default_port")]
    port: u16,
    preset: Vec<BuiltinPreset>,
}

fn default_port() -> u16 {
    8787
}

pub struct BuiltinData {
    pub presets: Vec<BuiltinPreset>,
    pub port: u16,
}

const FILE: &str = include_str!("../data/builtin-presets.toml");

/// Parse the embedded file. Failure is a build bug — callers treat it as
/// fatal (spec §5.2).
pub fn parse() -> anyhow::Result<BuiltinData> {
    let f: File = toml::from_str(FILE)?;
    Ok(BuiltinData { presets: f.preset, port: f.port })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_file_parses_with_the_r9700_presets() {
        let d = parse().unwrap();
        let v = &d.presets;
        assert_eq!(d.port, 8787);
        assert_eq!(v.len(), 4);
        assert_eq!(v[0].name, "R9700 220W -70mV 1350MHz VRAM");
        assert_eq!(v[0].settings.power_cap_w, Some(220.0));
        assert_eq!(v[0].settings.voltage_offset_mv, Some(-70));
        assert_eq!(v[0].settings.max_mclk_mhz, Some(1350));
        for (name, cap) in [
            ("R9700 210W -70mV", 210.0),
            ("R9700 220W -70mV", 220.0),
            ("R9700 225W -70mV", 225.0),
        ] {
            let p = v.iter().find(|p| p.name == name).unwrap();
            assert_eq!(p.settings.power_cap_w, Some(cap));
            assert_eq!(p.settings.voltage_offset_mv, Some(-70));
            assert_eq!(p.settings.max_mclk_mhz, None, "{name}: stock RAM");
        }
    }
}
