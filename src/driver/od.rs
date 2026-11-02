use std::collections::BTreeMap;

use super::Gpu;

/// Ranges from the `OD_RANGE:` section (min, max). `None` = the range line is
/// absent from the table — `apply_od` refuses to check against a guess
/// (spec §12: missing range data → refuse, don't guess).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct OdRanges {
    pub sclk_offset: Option<(i32, i32)>,
    pub mclk: Option<(i32, i32)>,
    pub vddgfx: Option<(i32, i32)>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct OdTable {
    pub sclk_offset_mhz: i32,
    /// slot index -> MHz (`0` = min/legacy base, `1` = max on RDNA4)
    pub mclk: BTreeMap<i32, i32>,
    pub vddgfx_mv: i32,
    pub ranges: OdRanges,
}

fn parse_mhz(tok: &str) -> anyhow::Result<i32> {
    let t = tok.trim().to_ascii_lowercase();
    let t = t.strip_suffix("mhz").unwrap_or(&t);
    t.trim()
        .parse::<i32>()
        .map_err(|_| anyhow::anyhow!("bad MHz token {tok:?}"))
}

fn parse_mv(tok: &str) -> anyhow::Result<i32> {
    let t = tok.trim().to_ascii_lowercase();
    let t = t.strip_suffix("mv").unwrap_or(&t);
    t.trim()
        .parse::<i32>()
        .map_err(|_| anyhow::anyhow!("bad mV token {tok:?}"))
}

/// Parse the RDNA4 `pp_od_clk_voltage` table.
///
/// Real-world quirks handled (see the checked-in fixture):
/// - the file may be NUL-padded (Sysfs strips NULs; parsers also ignore
///   blank fragments),
/// - the `OD_RANGE:` section header may be **absent** — the range lines
///   (`SCLK_OFFSET: lo hi`, `MCLK: lo hi`, `VDDGFX_OFFSET: lo hi`) are
///   recognized by their key anywhere in the file,
/// - section values and range lines interleave freely.
pub fn parse_table(raw: &str) -> anyhow::Result<OdTable> {
    let mut section = String::new();
    let mut sclk: Option<i32> = None;
    let mut mclk: BTreeMap<i32, i32> = BTreeMap::new();
    let mut vddgfx: Option<i32> = None;
    let mut ranges = OdRanges {
        sclk_offset: None,
        mclk: None,
        vddgfx: None,
    };

    for raw_line in raw.lines() {
        // NUL-stripping (done by the Sysfs layer) leaves one logical item
        // per line; the wide padding is intra-line spacing only.
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        // Range line: "KEY:  lo  hi" — recognized by key, anywhere in the
        // file (the `OD_RANGE:` header may be absent).
        if let Some((key, rest)) = line.split_once(':') {
            let key = key.trim();
            if matches!(key, "SCLK_OFFSET" | "MCLK" | "VDDGFX_OFFSET") {
                let nums: Vec<i32> = rest
                    .split_whitespace()
                    .map(|t| {
                        if t.to_ascii_lowercase().ends_with("mv") {
                            parse_mv(t)
                        } else {
                            parse_mhz(t)
                        }
                    })
                    .collect::<anyhow::Result<_>>()?;
                if nums.len() != 2 {
                    return Err(anyhow::anyhow!("OD_RANGE {key}: expected 2 values"));
                }
                match key {
                    "SCLK_OFFSET" => ranges.sclk_offset = Some((nums[0], nums[1])),
                    "MCLK" => ranges.mclk = Some((nums[0], nums[1])),
                    "VDDGFX_OFFSET" => ranges.vddgfx = Some((nums[0], nums[1])),
                    _ => unreachable!(),
                }
                continue;
            }
        }
        // Section header: any "NAME:" line switches the section. Unknown
        // sections (e.g. `OD_SCLK` or `VDDC_CURVE` on a VBIOS that exposes
        // more than this card — spec §20) become inactive sections and their
        // value lines are ignored, never mis-parsed as the previous
        // section's values.
        if let Some(h) = line.strip_suffix(':') {
            if !h.contains(' ') {
                section = h.trim().to_string();
                continue;
            }
        }
        // Value line for the current section.
        match section.as_str() {
            "OD_SCLK_OFFSET" => sclk = Some(parse_mhz(line)?),
            "OD_MCLK" => {
                // "0: 97Mhz"
                let (idx, val) = line
                    .split_once(':')
                    .ok_or_else(|| anyhow::anyhow!("bad OD_MCLK line {line:?}"))?;
                mclk.insert(idx.trim().parse()?, parse_mhz(val)?);
            }
            "OD_VDDGFX_OFFSET" => vddgfx = Some(parse_mv(line)?),
            _ => {}
        }
    }

    Ok(OdTable {
        sclk_offset_mhz: sclk.ok_or_else(|| anyhow::anyhow!("OD_SCLK_OFFSET missing"))?,
        mclk,
        vddgfx_mv: vddgfx.ok_or_else(|| anyhow::anyhow!("OD_VDDGFX_OFFSET missing"))?,
        ranges,
    })
}

impl Gpu {
    /// Apply overdrive fields with sparse-overlay semantics (spec §9): unset
    /// fields are re-staged at their *current committed* values, so a partial
    /// apply never wipes sibling OD fields. Writes `r` (reset), then `s` →
    /// `m 0` → `m 1` → `vo` (all three, effective values), then one `c`.
    pub async fn apply_od(
        &self,
        sclk_off: Option<i32>,
        max_mclk: Option<i32>,
        vddgfx_mv: Option<i32>,
    ) -> anyhow::Result<()> {
        let table_rel = self.rel("pp_od_clk_voltage");
        let raw = self
            .root
            .read_all(&table_rel)
            .await
            .map_err(|_| anyhow::anyhow!("no overdrive table at {table_rel}"))?;
        let tbl = parse_table(&raw)?;

        // Fail-closed range checks on the requested values only (committed
        // values are in range by construction). Missing range → refuse.
        if let Some(v) = sclk_off {
            let (lo, hi) = *tbl
                .ranges
                .sclk_offset
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("missing OD_RANGE SCLK_OFFSET"))?;
            if !(lo..=hi).contains(&v) {
                return Err(anyhow::anyhow!("sclk offset {v} out of range {lo}..{hi}"));
            }
        }
        if let Some(v) = max_mclk {
            let (lo, hi) = *tbl
                .ranges
                .mclk
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("missing OD_RANGE MCLK"))?;
            if !(lo..=hi).contains(&v) {
                return Err(anyhow::anyhow!("max mclk {v} out of range {lo}..{hi}"));
            }
        }
        if let Some(v) = vddgfx_mv {
            let (lo, hi) = *tbl
                .ranges
                .vddgfx
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("missing OD_RANGE VDDGFX_OFFSET"))?;
            if !(lo..=hi).contains(&v) || v > 0 {
                return Err(anyhow::anyhow!(
                    "vddgfx offset {v} invalid (range {lo}..{hi}, negatives only)"
                ));
            }
        }

        // Sparse merge: effective value per field = requested, else the
        // currently committed value (spec §9 "unset fields untouched").
        let sclk_v = sclk_off.unwrap_or(tbl.sclk_offset_mhz);
        let vdd_v = vddgfx_mv.unwrap_or(tbl.vddgfx_mv);

        // Reset staged table to defaults first (spec §6.3 step 3).
        self.root.write(&table_rel, "r").await?;
        self.root.write(&table_rel, &format!("s {sclk_v}")).await?;

        if let Some(mclk_v) = max_mclk.or_else(|| tbl.mclk.get(&1).copied()) {
            // Spec §6.2: min stays at the committed default (slot 0). Quirk:
            // if the min sits above the currently committed max, write the
            // max line first — the driver rejects the other order.
            let min_v = tbl.mclk.get(&0).copied().unwrap_or(0);
            let cur_max = tbl.mclk.get(&1).copied().unwrap_or(0);
            let min_line = format!("m 0 {min_v}");
            let max_line = format!("m 1 {mclk_v}");
            if min_v > cur_max {
                self.root.write(&table_rel, &max_line).await?;
                self.root.write(&table_rel, &min_line).await?;
            } else {
                self.root.write(&table_rel, &min_line).await?;
                self.root.write(&table_rel, &max_line).await?;
            }
        }

        self.root.write(&table_rel, &format!("vo {vdd_v}")).await?;
        self.root.write(&table_rel, "c").await?;
        Ok(())
    }

    pub async fn read_od(&self) -> anyhow::Result<OdTable> {
        let raw = self.root.read_all(&self.rel("pp_od_clk_voltage")).await?;
        parse_table(&raw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::driver::sysfs::MemSysfs;
    use std::collections::BTreeMap;
    use std::sync::Arc;

    const TABLE: &str = "OD_SCLK_OFFSET:\n0Mhz\nOD_MCLK:\n0: 97Mhz\n1: 1258MHz\nOD_VDDGFX_OFFSET:\n0mV\nOD_RANGE:\nSCLK_OFFSET:    -500Mhz       1000Mhz\nMCLK:      97Mhz       1500Mhz\nVDDGFX_OFFSET:    -200mv          0mv";

    fn table(s: &str) -> OdTable {
        parse_table(s).unwrap()
    }

    #[test]
    fn parse_fixture_table() {
        let t = table(TABLE);
        assert_eq!(t.sclk_offset_mhz, 0);
        assert_eq!(t.mclk, BTreeMap::from([(0, 97), (1, 1258)]));
        assert_eq!(t.vddgfx_mv, 0);
        assert_eq!(t.ranges.sclk_offset, Some((-500, 1000)));
        assert_eq!(t.ranges.mclk, Some((97, 1500)));
        assert_eq!(t.ranges.vddgfx, Some((-200, 0)));
    }

    #[test]
    fn parse_real_nul_padded_fixture() {
        // The checked-in real capture: NUL-padded, committed vddgfx offset
        // of -50mV (the real capture *does* carry an `OD_RANGE:` header; the
        // parser tolerates its absence — see missing_range_refused).
        let raw = std::fs::read_to_string("fixtures/sysfs/card1/device/pp_od_clk_voltage")
            .unwrap()
            .replace('\0', "");
        let t = table(&raw);
        assert_eq!(t.sclk_offset_mhz, 0);
        assert_eq!(t.mclk, BTreeMap::from([(0, 97), (1, 1259)]));
        assert_eq!(t.vddgfx_mv, -50);
        assert_eq!(t.ranges.sclk_offset, Some((-500, 1000)));
        assert_eq!(t.ranges.mclk, Some((97, 1500)));
        assert_eq!(t.ranges.vddgfx, Some((-200, 0)));
    }

    #[test]
    fn parse_rejects_garbage() {
        assert!(parse_table("not a table").is_err());
        assert!(parse_table("").is_err());
    }

    async fn test_gpu(files: &[(&str, &str)]) -> (Gpu, MemSysfs) {
        let s = MemSysfs::new();
        for (k, v) in files {
            s.put(k, v).await;
        }
        let g = Gpu {
            root: Arc::new(s.clone()),
            rel: "card0".into(),
            slot: "1002:75b4-1458:242f-0000:0b:00.0".into(),
            card_n: 0,
            hwmon: "hwmon0".into(),
        };
        (g, s)
    }

    fn seq(s: &MemSysfs) -> Vec<String> {
        s.write_log().into_iter().map(|(_, v)| v).collect()
    }

    #[tokio::test]
    async fn apply_all_fields_orders_and_commits() {
        let (g, s) = test_gpu(&[("card0/device/pp_od_clk_voltage", TABLE)]).await;
        g.apply_od(Some(100), Some(1150), Some(-50)).await.unwrap();
        // Full protocol: reset, staged lines (s → m 0 → m 1 → vo), commit.
        assert_eq!(
            seq(&s),
            vec!["r", "s 100", "m 0 97", "m 1 1150", "vo -50", "c"]
        );
    }

    #[tokio::test]
    async fn apply_refuses_out_of_range_sclk() {
        let (g, s) = test_gpu(&[("card0/device/pp_od_clk_voltage", TABLE)]).await;
        let err = g.apply_od(Some(2000), None, None).await.unwrap_err();
        assert!(err.to_string().contains("out of range"));
        assert!(s.write_log().is_empty(), "refusal must not write anything");
    }

    #[tokio::test]
    async fn apply_positive_voltage_refused() {
        let (g, s) = test_gpu(&[("card0/device/pp_od_clk_voltage", TABLE)]).await;
        let err = g.apply_od(None, None, Some(50)).await.unwrap_err();
        assert!(err.to_string().contains("negatives only"));
        assert!(s.write_log().is_empty(), "refusal must not write anything");
    }

    #[tokio::test]
    async fn apply_max_mclk_at_boundary_ok() {
        // Range is 97..=1500; the boundary value is legal; min stays default.
        // Sparse merge: unset fields are re-staged at their committed values
        // (sclk 0, vddgfx 0 here) — see the interface note.
        let (g, s) = test_gpu(&[("card0/device/pp_od_clk_voltage", TABLE)]).await;
        g.apply_od(None, Some(97), None).await.unwrap();
        assert_eq!(seq(&s), vec!["r", "s 0", "m 0 97", "m 1 97", "vo 0", "c"]);
    }

    #[tokio::test]
    async fn missing_table_refuses() {
        let (g, s) = test_gpu(&[]).await;
        let err = g.apply_od(Some(100), None, None).await.unwrap_err();
        assert!(err.to_string().contains("no overdrive table"));
        assert!(s.write_log().is_empty(), "refusal must not write anything");
    }

    #[tokio::test]
    async fn missing_range_refused() {
        // Spec §18: a table without `OD_RANGE` lines is refused (fail-closed,
        // never guess bounds) — even a zero offset, which a (0,0) default
        // range would have silently accepted.
        let no_ranges =
            "OD_SCLK_OFFSET:\n0Mhz\nOD_MCLK:\n0: 97Mhz\n1: 1258MHz\nOD_VDDGFX_OFFSET:\n0mV";
        let t = table(no_ranges);
        assert_eq!(t.ranges.sclk_offset, None);
        let (g, s) = test_gpu(&[("card0/device/pp_od_clk_voltage", no_ranges)]).await;
        let err = g.apply_od(Some(0), None, None).await.unwrap_err();
        assert!(err.to_string().contains("missing OD_RANGE SCLK_OFFSET"));
        assert!(s.write_log().is_empty(), "refusal must not write anything");
    }

    #[tokio::test]
    async fn min_above_max_reorders_lines() {
        // Spec §18: the RDNA3/4 min>max quirk — when the committed min sits
        // above the committed max, the max line must precede the min line.
        let crossed = "OD_SCLK_OFFSET:\n0Mhz\nOD_MCLK:\n0: 2000Mhz\n1: 1000Mhz\nOD_VDDGFX_OFFSET:\n0mV\nOD_RANGE:\nSCLK_OFFSET:    -500Mhz       1000Mhz\nMCLK:      100Mhz       2500Mhz\nVDDGFX_OFFSET:    -200mv          0mv";
        let (g, s) = test_gpu(&[("card0/device/pp_od_clk_voltage", crossed)]).await;
        g.apply_od(None, Some(1250), None).await.unwrap();
        assert_eq!(
            seq(&s),
            vec!["r", "s 0", "m 1 1250", "m 0 2000", "vo 0", "c"]
        );
    }
}
