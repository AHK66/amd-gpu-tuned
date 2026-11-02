// src/driver/fixture.rs — enumerate GPUs from a captured sysfs fixture tree
// (`fixtures/sysfs`). Backs the `--fake-sysfs` flag so e2e tests run the real
// daemon + SPA with no hardware (repo invariant: no test touches real hardware).
use std::path::Path;
use std::sync::Arc;

use super::fake::FakeSysfs;
use super::Gpu;

fn walk(dir: &Path, prefix: &str, out: &mut Vec<(String, String)>) -> anyhow::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let rel = format!("{prefix}/{}", entry.file_name().to_string_lossy());
        if path.is_dir() {
            walk(&path, &rel, out)?;
        } else {
            // Some sysfs files are binary blobs (e.g. `pp_table`); the daemon
            // never reads them, so skip anything that isn't valid UTF-8.
            let bytes = std::fs::read(&path)?;
            match String::from_utf8(bytes) {
                Ok(s) => out.push((rel, s)),
                Err(_) => tracing::debug!("fixture: skipping binary file {rel}"),
            }
        }
    }
    Ok(())
}

/// Load every `cardN` directory under `root` as a `Gpu` backed by a
/// `FakeSysfs` (same amdgpu staging/commit semantics the unit tests use).
pub fn load_fixture(root: &Path) -> anyhow::Result<Vec<Gpu>> {
    let mut cards: Vec<String> = std::fs::read_dir(root)?
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| {
            n.starts_with("card")
                && n["card".len()..].chars().all(|c| c.is_ascii_digit())
        })
        .collect();
    cards.sort();

    let mut out = Vec::new();
    for name in &cards {
        let card_root = root.join(name);
        // Paths are relative to the fixture root so FakeSysfs keys match the
        // production layout ("cardN/device/...").
        let mut flat: Vec<(String, String)> = Vec::new();
        walk(&card_root, name, &mut flat)?;
        let refs: Vec<(&str, &str)> =
            flat.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        let fs = FakeSysfs::with(&refs);

        let uevent = flat
            .iter()
            .find(|(k, _)| k == &format!("{name}/device/uevent"))
            .map(|(_, v)| v.clone())
            .ok_or_else(|| anyhow::anyhow!("{name}: fixture missing device/uevent"))?;
        let slot = Gpu::slot_from_uevent(&uevent)?;
        let card_n = name["card".len()..].parse::<u32>().unwrap_or(0);
        let hwmon = std::fs::read_dir(card_root.join("device/hwmon"))
            .ok()
            .and_then(|mut d| d.next().and_then(Result::ok))
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .ok_or_else(|| anyhow::anyhow!("{name}: fixture missing device/hwmon"))?;

        out.push(Gpu {
            root: Arc::new(fs),
            rel: name.clone(),
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

    #[test]
    fn load_fixture_enumerates_card1() {
        let gpus = load_fixture(Path::new("fixtures/sysfs")).unwrap();
        assert_eq!(gpus.len(), 1);
        assert_eq!(gpus[0].slot, "1002:7550-148c:2435-0000:0f:00.0");
        assert_eq!(gpus[0].card_n, 1);
        assert_eq!(gpus[0].hwmon, "hwmon5");
        assert_eq!(gpus[0].rel, "card1");
    }

    #[tokio::test]
    async fn fixture_gpu_reads_real_values() {
        let gpus = load_fixture(Path::new("fixtures/sysfs")).unwrap();
        let cap = gpus[0].read_power_cap_w().await.unwrap();
        assert!(cap > 0.0, "power cap should read from the fixture, got {cap}");
        let od = gpus[0].read_od().await.unwrap();
        assert_eq!(od.sclk_offset_mhz, 0); // captured stock state
    }

    // Proves `FakeSysfs` is card-agnostic: card1's OD table must be a *staged*
    // table (apply + readback verify), not a plain file. Fails while
    // `FakeSysfs::with` is hardcoded to `card0` (card1's OD then degrades to a
    // plain file and the apply/readback roundtrip breaks).
    #[tokio::test]
    async fn fixture_gpu_od_apply_roundtrip() {
        let gpus = load_fixture(Path::new("fixtures/sysfs")).unwrap();
        gpus[0].apply_od(Some(50), None, None).await.unwrap();
        let od = gpus[0].read_od().await.unwrap();
        assert_eq!(od.sclk_offset_mhz, 50);
    }
}
