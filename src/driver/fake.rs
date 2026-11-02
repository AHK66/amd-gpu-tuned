// src/driver/fake.rs — in-memory driver emulator for tests.
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;

use super::od::OdTable;
use super::sysfs::Sysfs;
#[cfg(test)]
use super::Gpu;

struct OdState {
    committed: OdTable,
    staged: Option<StagedOd>,
    defaults: OdTable, // what `r` restores
}

#[derive(Clone)]
struct StagedOd {
    sclk: Option<i32>,
    mclk: Option<BTreeMap<i32, i32>>,
    vddgfx: Option<i32>,
}

struct FanState {
    committed: Vec<(i32, f32)>,
    committed_range: ((i32, i32), (f32, f32)),
    staged: Option<Vec<(i32, f32)>>,
}

struct Inner {
    files: HashMap<PathBuf, String>,
    od: Option<OdState>,
    fan: Option<FanState>,
}

#[derive(Clone)]
pub struct FakeSysfs {
    inner: Arc<Mutex<Inner>>,
    od_rel: PathBuf,
    fan_rel: PathBuf,
}

impl FakeSysfs {
    // Sync on purpose (no await in the body — an `async fn` would trip
    // clippy::unused_async at the per-commit clippy gate).
    pub fn with(files: &[(&str, &str)]) -> Self {
        // Card-agnostic: match by suffix so fixtures captured from any cardN
        // (e.g. `card1`) get the same staged-table semantics as `card0`.
        let od_rel = files
            .iter()
            .find(|(k, _)| k.ends_with("/device/pp_od_clk_voltage"))
            .map(|(k, _)| PathBuf::from(*k))
            .unwrap_or_else(|| PathBuf::from("card0/device/pp_od_clk_voltage"));
        let fan_rel = files
            .iter()
            .find(|(k, _)| k.ends_with("/device/gpu_od/fan_ctrl/fan_curve"))
            .map(|(k, _)| PathBuf::from(*k))
            .unwrap_or_else(|| PathBuf::from("card0/device/gpu_od/fan_ctrl/fan_curve"));
        let mut map = HashMap::new();
        let mut od = None;
        let mut fan = None;
        for (k, v) in files {
            let p = PathBuf::from(*k);
            if p == od_rel {
                if let Ok(t) = super::od::parse_table(&super::sysfs::clean(v)) {
                    od = Some(OdState {
                        defaults: t.clone(),
                        committed: t,
                        staged: None,
                    });
                    continue;
                }
            }
            if p == fan_rel {
                if let Ok(c) = super::fan::parse_curve(&super::sysfs::clean(v)) {
                    fan = Some(FanState {
                        committed_range: (c.t_range, c.p_range),
                        committed: c.points,
                        staged: None,
                    });
                    continue;
                }
            }
            map.insert(p, (*v).to_string());
        }
        Self {
            inner: Arc::new(Mutex::new(Inner {
                files: map,
                od,
                fan,
            })),
            od_rel,
            fan_rel,
        }
    }

    fn render_od(t: &OdTable) -> String {
        let mut s = String::new();
        s.push_str(&format!("OD_SCLK_OFFSET:\n{}Mhz\n", t.sclk_offset_mhz));
        s.push_str("OD_MCLK:\n");
        for (i, v) in &t.mclk {
            s.push_str(&format!("{i}: {v}MHz\n"));
        }
        s.push_str(&format!("OD_VDDGFX_OFFSET:\n{}mV\n", t.vddgfx_mv));
        if let Some((lo, hi)) = t.ranges.sclk_offset {
            s.push_str(&format!(
                "OD_RANGE:\nSCLK_OFFSET:    {lo}Mhz       {hi}Mhz\n"
            ));
        }
        if let Some((lo, hi)) = t.ranges.mclk {
            s.push_str(&format!("MCLK:      {lo}Mhz       {hi}Mhz\n"));
        }
        if let Some((lo, hi)) = t.ranges.vddgfx {
            s.push_str(&format!("VDDGFX_OFFSET:    {lo}mv          {hi}mv\n"));
        }
        s
    }

    fn render_fan(state: &FanState) -> String {
        let mut s = String::from("OD_FAN_CURVE:\n");
        for (i, (t, p)) in state.committed.iter().enumerate() {
            s.push_str(&format!("{i}: {t}C {0:.0}%\n", p * 100.0));
        }
        s.push_str("OD_RANGE:\n");
        s.push_str(&format!(
            "FAN_CURVE(hotspot temp): {}C {}C\n",
            state.committed_range.0 .0, state.committed_range.0 .1
        ));
        s.push_str(&format!(
            "FAN_CURVE(fan speed): {}% {}%\n",
            (state.committed_range.1 .0 * 100.0) as i32,
            (state.committed_range.1 .1 * 100.0) as i32
        ));
        s
    }
}

#[async_trait]
impl Sysfs for FakeSysfs {
    async fn read(&self, rel: &str) -> anyhow::Result<String> {
        self.read_all(rel).await
    }

    async fn read_all(&self, rel: &str) -> anyhow::Result<String> {
        let inner = self.inner.lock().unwrap();
        let p = PathBuf::from(rel);
        if p == self.od_rel {
            if let Some(st) = &inner.od {
                return Ok(Self::render_od(&st.committed));
            }
        }
        if p == self.fan_rel {
            if let Some(st) = &inner.fan {
                return Ok(Self::render_fan(st));
            }
        }
        // Same contract as FsSysfs/MemSysfs: NULs stripped, trimmed.
        inner
            .files
            .get(&p)
            .cloned()
            .map(|s| super::sysfs::clean(&s).trim().to_string())
            .ok_or_else(|| anyhow::anyhow!("read {rel}: No such file or directory"))
    }

    async fn write(&self, rel: &str, content: &str) -> anyhow::Result<()> {
        let content = content.trim();
        let mut inner = self.inner.lock().unwrap();
        let p = PathBuf::from(rel);
        if p == self.od_rel {
            let st = inner.od.as_mut().expect("od table not seeded");
            match content {
                "r" => {
                    st.staged = Some(StagedOd {
                        sclk: Some(st.defaults.sclk_offset_mhz),
                        mclk: Some(st.defaults.mclk.clone()),
                        vddgfx: Some(st.defaults.vddgfx_mv),
                    })
                }
                "c" => {
                    if let Some(s) = st.staged.take() {
                        if let Some(v) = s.sclk {
                            st.committed.sclk_offset_mhz = v;
                        }
                        if let Some(m) = s.mclk {
                            st.committed.mclk = m;
                        }
                        if let Some(v) = s.vddgfx {
                            st.committed.vddgfx_mv = v;
                        }
                    }
                }
                line => {
                    let staged = st.staged.get_or_insert(StagedOd {
                        sclk: None,
                        mclk: None,
                        vddgfx: None,
                    });
                    // Staged lines are unitless integers (golden-snapshot grammar).
                    if let Some(v) = line.strip_prefix("s ") {
                        staged.sclk = Some(v.parse()?);
                    } else if let Some(rest) = line.strip_prefix("m ") {
                        let (idx, val) = rest.split_once(' ').unwrap();
                        staged
                            .mclk
                            .get_or_insert_with(|| st.committed.mclk.clone())
                            .insert(idx.parse()?, val.parse()?);
                    } else if let Some(v) = line.strip_prefix("vo ") {
                        staged.vddgfx = Some(v.parse()?);
                    }
                }
            }
            return Ok(());
        }
        if p == self.fan_rel {
            let st = inner.fan.as_mut().expect("fan curve not seeded");
            match content {
                "c" => {
                    if let Some(s) = st.staged.take() {
                        st.committed = s;
                    }
                }
                line => {
                    // "i t p" — unitless (golden-snapshot grammar)
                    let mut it = line.split_whitespace();
                    if let (Some(i), Some(t), Some(p)) = (it.next(), it.next(), it.next()) {
                        let i: usize = i.parse()?;
                        let t: i32 = t.parse()?;
                        let p: f32 = p.parse::<f32>()? / 100.0;
                        let staged = st.staged.get_or_insert_with(Vec::new);
                        if staged.len() <= i {
                            staged.resize(i + 1, (0, 0.0));
                        }
                        staged[i] = (t, p);
                    }
                }
            }
            return Ok(());
        }
        inner.files.insert(p, content.to_string());
        Ok(())
    }

    async fn exists(&self, rel: &str) -> bool {
        let inner = self.inner.lock().unwrap();
        let p = PathBuf::from(rel);
        (p == self.od_rel && inner.od.is_some())
            || (p == self.fan_rel && inner.fan.is_some())
            || inner.files.contains_key(&p)
    }

    async fn list_dir(&self, rel: &str) -> anyhow::Result<Vec<String>> {
        let inner = self.inner.lock().unwrap();
        let prefix = rel.trim_end_matches('/').to_string();
        let mut out: Vec<String> = inner
            .files
            .keys()
            .filter_map(|p| {
                let s = p.to_string_lossy();
                s.strip_prefix(&format!("{prefix}/"))
                    .and_then(|rest| rest.split_once('/').map(|(first, _)| first.to_string()))
            })
            .collect();
        out.sort();
        out.dedup();
        Ok(out)
    }
}

// Kept `async` so existing call sites (`gpu(…).await`) stay uniform;
// the body itself has no await.
#[cfg(test)]
#[allow(clippy::unused_async)]
pub async fn gpu(files: &[(&str, &str)]) -> Gpu {
    gpu_sync(files)
}

/// Sync variant for callers that already run on an executor (telemetry
/// tests, `tests_common`) — same body, no `async` wrapper.
#[cfg(test)]
pub fn gpu_sync(files: &[(&str, &str)]) -> Gpu {
    let root = FakeSysfs::with(files);
    Gpu {
        root: Arc::new(root),
        rel: "card0".into(),
        slot: "1002:75b4-1458:242f-0000:0b:00.0".into(),
        card_n: 0,
        hwmon: "hwmon0".into(),
    }
}

/// Like [`gpu_sync`] but with an explicit slot id (multi-GPU tests).
#[cfg(test)]
pub fn gpu_with_slot(files: &[(&str, &str)], slot: &str) -> Gpu {
    let mut g = gpu_sync(files);
    g.slot = slot.to_string();
    g
}

#[cfg(test)]
mod tests {
    use super::*;

    const OD: &str = "OD_SCLK_OFFSET:\n0Mhz\nOD_MCLK:\n0: 97Mhz\n1: 1258MHz\nOD_VDDGFX_OFFSET:\n0mV\nOD_RANGE:\nSCLK_OFFSET:    -500Mhz       1000Mhz\nMCLK:      97Mhz       1500Mhz\nVDDGFX_OFFSET:    -200mv          0mv";

    const OD_REL: &str = "card0/device/pp_od_clk_voltage";

    #[tokio::test]
    async fn od_roundtrip_applies_and_renders() {
        let g = gpu(&[(OD_REL, OD)]).await;
        g.apply_od(Some(100), Some(1150), Some(-50)).await.unwrap();
        let t = g.read_od().await.unwrap();
        assert_eq!(t.sclk_offset_mhz, 100);
        assert_eq!(t.mclk[&1], 1150);
        assert_eq!(t.mclk[&0], 97); // min untouched
        assert_eq!(t.vddgfx_mv, -50);
    }

    #[tokio::test]
    async fn od_reset_restores_defaults() {
        let g = gpu(&[(OD_REL, OD)]).await;
        g.apply_od(Some(100), None, None).await.unwrap();
        // A bare `r` + `c` sequence resets.
        g.root.write(OD_REL, "r").await.unwrap();
        g.root.write(OD_REL, "c").await.unwrap();
        let t = g.read_od().await.unwrap();
        assert_eq!(t.sclk_offset_mhz, 0);
    }

    #[tokio::test]
    async fn plain_file_readback() {
        let g = gpu(&[("card0/device/hwmon/hwmon0/power1_cap", "300000000")]).await;
        g.root
            .write("card0/device/hwmon/hwmon0/power1_cap", "235000000")
            .await
            .unwrap();
        assert_eq!(
            g.root
                .read("card0/device/hwmon/hwmon0/power1_cap")
                .await
                .unwrap(),
            "235000000"
        );
    }

    #[tokio::test]
    async fn unknown_write_on_staged_table_is_ignored() {
        // Garbage on a staged table must not corrupt state (defensive).
        let g = gpu(&[(OD_REL, OD)]).await;
        g.root.write(OD_REL, "gibberish").await.unwrap();
        let t = g.read_od().await.unwrap();
        assert_eq!(t.sclk_offset_mhz, 0); // unchanged, no commit happened
    }
}
