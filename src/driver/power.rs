use super::Gpu;
use crate::types::ApplyOutcome;

impl Gpu {
    async fn cap_bounds(&self) -> anyhow::Result<(i32, i32)> {
        let lo = self
            .root
            .read(&self.h("power1_cap_min"))
            .await?
            .parse::<i32>()?;
        let hi = self
            .root
            .read(&self.h("power1_cap_max"))
            .await?
            .parse::<i32>()?;
        Ok((lo, hi))
    }

    // Spec note: §9 says the power cap is "clamped", but §6.3/§12 are
    // fail-closed ("refuse out-of-range with an error"). We follow §6.3 —
    // an out-of-range cap is refused per-field (below), never silently
    // clamped.
    pub async fn apply_power_cap(&self, w: f64) -> anyhow::Result<ApplyOutcome> {
        let (lo, hi) = self.cap_bounds().await?;
        let uw = (w * 1e6) as i64;
        if !(lo as i64..=hi as i64).contains(&uw) {
            return Ok(ApplyOutcome {
                field: "power_cap_w".into(),
                status: "failed".into(),
                detail: format!("out of range {}..{} W", lo as f64 / 1e6, hi as f64 / 1e6),
            });
        }
        let rel = self.h("power1_cap");
        self.root.write(&rel, &uw.to_string()).await?;
        match self.root.read(&rel).await {
            Ok(back) => {
                let bw: i64 = back.parse().unwrap_or(uw);
                let ok = (bw - uw).abs() <= 1_000_000; // within 1 W
                Ok(ApplyOutcome {
                    field: "power_cap_w".into(),
                    status: if ok { "verified" } else { "applied" }.into(),
                    detail: format!("cap set to {w} W (readback {bw} uW)"),
                })
            }
            Err(_) => Ok(ApplyOutcome {
                field: "power_cap_w".into(),
                status: "applied".into(),
                detail: format!("cap set to {w} W (no readback)"),
            }),
        }
    }

    pub async fn set_dpm_force(&self, level: &str) -> anyhow::Result<()> {
        self.root
            .write(&self.rel("power_dpm_force_performance_level"), level)
            .await?;
        Ok(())
    }

    pub async fn read_power_cap_w(&self) -> Option<f64> {
        self.root
            .read(&self.h("power1_cap"))
            .await
            .ok()
            .and_then(|s| s.parse::<i64>().ok())
            .map(|u| u as f64 / 1e6)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::driver::sysfs::{MemSysfs, Sysfs};
    use std::sync::Arc;

    /// Returns the GPU handle *and* the backing MemSysfs (the `Sysfs` trait
    /// object exposes no `put`; tests that seed extra files need the handle).
    async fn pow_gpu() -> (Gpu, MemSysfs) {
        let s = MemSysfs::new();
        s.put("card0/device/hwmon/hwmon0/power1_cap_min", "120000000")
            .await;
        s.put("card0/device/hwmon/hwmon0/power1_cap_max", "300000000")
            .await;
        s.put("card0/device/hwmon/hwmon0/power1_cap", "300000000")
            .await;
        let g = Gpu {
            root: Arc::new(s.clone()),
            rel: "card0".into(),
            slot: "s".into(),
            card_n: 0,
            hwmon: "hwmon0".into(),
        };
        (g, s)
    }

    #[tokio::test]
    async fn apply_cap_verified_on_readback() {
        let (g, _) = pow_gpu().await;
        let out = g.apply_power_cap(235.0).await.unwrap();
        assert_eq!(out.status, "verified");
        assert_eq!(
            g.root
                .read("card0/device/hwmon/hwmon0/power1_cap")
                .await
                .unwrap(),
            "235000000"
        );
    }

    #[tokio::test]
    async fn apply_cap_refuses_out_of_range() {
        let (g, _) = pow_gpu().await;
        let out = g.apply_power_cap(5.0).await.unwrap();
        assert_eq!(out.status, "failed");
        assert!(out.detail.contains("out of range"));
    }

    #[tokio::test]
    async fn set_dpm_force() {
        let (g, s) = pow_gpu().await;
        s.put("card0/device/power_dpm_force_performance_level", "manual")
            .await;
        g.set_dpm_force("auto").await.unwrap();
        assert_eq!(
            s.read("card0/device/power_dpm_force_performance_level")
                .await
                .unwrap(),
            "auto"
        );
    }

}
