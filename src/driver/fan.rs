use super::Gpu;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FanCurve {
    /// (degC, speed 0..=1), committed points, index order
    pub points: Vec<(i32, f32)>,
    pub t_range: (i32, i32),
    pub p_range: (f32, f32),
    /// Both `FAN_CURVE(...)` `OD_RANGE` lines were present.
    /// `#[serde(default)]` keeps pre-existing baseline JSON parseable.
    #[serde(default)]
    pub has_ranges: bool,
}

fn parse_pct(tok: &str) -> anyhow::Result<f32> {
    let t = tok.trim().strip_suffix('%').unwrap_or(tok);
    t.trim()
        .parse::<f32>()
        .map(|v| v / 100.0)
        .map_err(|_| anyhow::anyhow!("bad pct token {tok:?}"))
}

fn parse_c(tok: &str) -> anyhow::Result<i32> {
    let t = tok.trim().strip_suffix('C').unwrap_or(tok);
    t.trim()
        .parse::<i32>()
        .map_err(|_| anyhow::anyhow!("bad C token {tok:?}"))
}

pub fn parse_curve(raw: &str) -> anyhow::Result<FanCurve> {
    let mut points: Vec<(i32, f32)> = Vec::new();
    let mut t_range: (i32, i32) = (0, 0);
    let mut p_range: (f32, f32) = (0.0, 0.0);
    let (mut has_t_range, mut has_p_range) = (false, false);

    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // "0: 25C 30%" -> point
        if let Some((idx, rest)) = line.split_once(':') {
            if idx.trim().chars().all(|c| c.is_ascii_digit()) {
                let mut parts = rest.split_whitespace();
                if let (Some(t), Some(p)) = (parts.next(), parts.next()) {
                    points.push((parse_c(t)?, parse_pct(p)?));
                    continue;
                }
            }
        }
        if line.starts_with("FAN_CURVE(hotspot temp)") {
            let rest = line.split_once(':').map(|(_, r)| r).unwrap_or("");
            let mut it = rest.split_whitespace();
            if let (Some(a), Some(b)) = (it.next(), it.next()) {
                t_range = (parse_c(a)?, parse_c(b)?);
                has_t_range = true;
            }
        }
        if line.starts_with("FAN_CURVE(fan speed)") {
            let rest = line.split_once(':').map(|(_, r)| r).unwrap_or("");
            let mut it = rest.split_whitespace();
            if let (Some(a), Some(b)) = (it.next(), it.next()) {
                p_range = (parse_pct(a)?, parse_pct(b)?);
                has_p_range = true;
            }
        }
    }

    if points.is_empty() {
        return Err(anyhow::anyhow!("no fan curve points"));
    }
    Ok(FanCurve {
        points,
        t_range,
        p_range,
        has_ranges: has_t_range && has_p_range,
    })
}

impl Gpu {
    pub async fn read_fan_curve(&self) -> anyhow::Result<Option<FanCurve>> {
        let rel = self.rel("gpu_od/fan_ctrl/fan_curve");
        match self.root.read_all(&rel).await {
            // A readable file that doesn't parse as a table (e.g. the staged
            // commit marker under in-memory fakes) means "no usable readback".
            Ok(raw) => Ok(parse_curve(&raw).ok()),
            Err(_) => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CURVE: &str = "OD_FAN_CURVE:\n0: 25C 30%\n1: 50C 50%\n2: 75C 75%\n3: 100C 100%\nOD_RANGE:\nFAN_CURVE(hotspot temp): 25C 100C\nFAN_CURVE(fan speed): 30% 100%";

    #[test]
    fn parse_curve_table() {
        let c = parse_curve(CURVE).unwrap();
        assert_eq!(c.points, vec![(25, 0.3), (50, 0.5), (75, 0.75), (100, 1.0)]);
        assert_eq!(c.t_range, (25, 100));
        assert_eq!(c.p_range, (0.3, 1.0));
    }
}
