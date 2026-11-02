#[cfg(test)]
use std::collections::HashMap;
#[cfg(test)]
use std::path::Path;
use std::path::PathBuf;
#[cfg(test)]
use std::sync::{Arc, Mutex};

use async_trait::async_trait;

#[async_trait]
pub trait Sysfs: Send + Sync {
    /// Read a file, NULs stripped, trimmed.
    async fn read(&self, rel: &str) -> anyhow::Result<String>;
    async fn write(&self, rel: &str, content: &str) -> anyhow::Result<()>;
    async fn exists(&self, rel: &str) -> bool;
    /// Read a file, NULs stripped, trimmed, newlines kept.
    async fn read_all(&self, rel: &str) -> anyhow::Result<String>;
    /// Entry names under a directory (empty vec if the dir is absent).
    async fn list_dir(&self, rel: &str) -> anyhow::Result<Vec<String>>;
}

/// amdgpu sysfs tables are fixed-width, NUL-padded blocks — strip NULs and
/// other non-printables before handing text to parsers.
pub(super) fn clean(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii() && (c.is_ascii_graphic() || *c == ' ' || *c == '\n' || *c == '\t'))
        .collect()
}

/// Production implementation: reads/writes under `root` (normally
/// `/sys/class/drm`). I/O runs on the blocking pool.
pub struct FsSysfs {
    pub root: PathBuf,
}

#[async_trait]
impl Sysfs for FsSysfs {
    async fn read(&self, rel: &str) -> anyhow::Result<String> {
        let root = self.root.clone();
        let rel = rel.to_string();
        tokio::task::spawn_blocking(move || {
            let bytes = std::fs::read(root.join(&rel))?;
            let s = String::from_utf8_lossy(&bytes);
            Ok(clean(&s).trim().to_string())
        })
        .await?
    }

    async fn write(&self, rel: &str, content: &str) -> anyhow::Result<()> {
        let root = self.root.clone();
        let rel = rel.to_string();
        let content = content.to_string();
        tokio::task::spawn_blocking(move || {
            std::fs::write(root.join(&rel), content)
                .map_err(|e| anyhow::anyhow!("write {rel}: {e}"))
        })
        .await?
    }

    async fn exists(&self, rel: &str) -> bool {
        let root = self.root.clone();
        let rel = rel.to_string();
        tokio::task::spawn_blocking(move || root.join(&rel).exists())
            .await
            .unwrap_or(false)
    }

    async fn read_all(&self, rel: &str) -> anyhow::Result<String> {
        let root = self.root.clone();
        let rel = rel.to_string();
        tokio::task::spawn_blocking(move || {
            let bytes = std::fs::read(root.join(&rel))?;
            let s = String::from_utf8_lossy(&bytes);
            Ok(clean(&s).trim().to_string())
        })
        .await?
    }

    async fn list_dir(&self, rel: &str) -> anyhow::Result<Vec<String>> {
        let root = self.root.clone();
        let rel = rel.to_string();
        tokio::task::spawn_blocking(move || {
            let mut out = Vec::new();
            if let Ok(rd) = std::fs::read_dir(root.join(&rel)) {
                for e in rd.flatten() {
                    out.push(e.file_name().to_string_lossy().into_owned());
                }
            }
            Ok(out)
        })
        .await?
    }
}

/// In-memory tree for tests: same error semantics as the real sysfs.
/// Also records every write (rel, content) in order — protocol assertions
/// in Tasks 4–5 read `writes`.
#[cfg(test)]
#[derive(Default, Clone)]
pub struct MemSysfs {
    pub files: Arc<Mutex<HashMap<PathBuf, String>>>,
    pub writes: Arc<Mutex<Vec<(String, String)>>>,
}

#[cfg(test)]
impl MemSysfs {
    pub fn new() -> Self {
        Self::default()
    }
    #[allow(clippy::unused_async)]
    pub async fn put(&self, rel: &str, content: &str) {
        self.files
            .lock()
            .unwrap()
            .insert(PathBuf::from(rel), content.to_string());
    }
    #[allow(clippy::unused_async)]
    pub async fn get(&self, rel: &str) -> Option<String> {
        self.files.lock().unwrap().get(Path::new(rel)).cloned()
    }
    pub fn write_log(&self) -> Vec<(String, String)> {
        self.writes.lock().unwrap().clone()
    }
}

#[cfg(test)]
#[async_trait]
impl Sysfs for MemSysfs {
    async fn read(&self, rel: &str) -> anyhow::Result<String> {
        self.get(rel)
            .await
            .map(|v| clean(&v).trim().to_string())
            .ok_or_else(|| anyhow::anyhow!("read {rel}: No such file or directory"))
    }
    async fn write(&self, rel: &str, content: &str) -> anyhow::Result<()> {
        self.writes
            .lock()
            .unwrap()
            .push((rel.to_string(), content.to_string()));
        self.put(rel, content).await;
        Ok(())
    }
    async fn exists(&self, rel: &str) -> bool {
        self.get(rel).await.is_some()
    }
    async fn read_all(&self, rel: &str) -> anyhow::Result<String> {
        self.read(rel).await
    }
    async fn list_dir(&self, rel: &str) -> anyhow::Result<Vec<String>> {
        let prefix = rel.trim_end_matches('/').to_string();
        let files = self.files.lock().unwrap().clone();
        let mut out: Vec<String> = files
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn read_trims_single_line() {
        let s = MemSysfs::new();
        s.put("card0/device/uevent", "PCI_ID=1002:75B4\n").await;
        assert_eq!(
            s.read("card0/device/uevent").await.unwrap(),
            "PCI_ID=1002:75B4"
        );
    }

    #[tokio::test]
    async fn read_all_keeps_newlines() {
        let s = MemSysfs::new();
        s.put("card0/device/pp_dpm_sclk", "SCLK: 0: 400MHz 1: 1300MHz *\n")
            .await;
        assert_eq!(
            s.read_all("card0/device/pp_dpm_sclk").await.unwrap(),
            "SCLK: 0: 400MHz 1: 1300MHz *"
        );
    }

    #[tokio::test]
    async fn missing_file_errors() {
        let s = MemSysfs::new();
        assert!(s.read("nope").await.is_err());
    }

    #[tokio::test]
    async fn exists_and_write_roundtrip() {
        let s = MemSysfs::new();
        assert!(!s.exists("card0/x").await);
        s.write("card0/x", "280000000").await.unwrap();
        assert_eq!(s.read("card0/x").await.unwrap(), "280000000");
    }

    #[tokio::test]
    async fn fs_sysfs_reads_fixture_tree() {
        let s = FsSysfs {
            root: std::path::PathBuf::from("fixtures/sysfs"),
        };
        let uevent = s.read_all("card1/device/uevent").await.unwrap();
        assert!(uevent.contains("PCI_ID=1002:7550"));
        assert!(s.read("card1/device/hwmon/hwmon5/power1_cap").await.is_ok());
        assert!(s.exists("card1/device/gpu_od/fan_ctrl/fan_curve").await);
        assert!(!s.exists("card1/does-not-exist").await);
        let hwmons = s.list_dir("card1/device/hwmon").await.unwrap();
        assert_eq!(hwmons, vec!["hwmon5".to_string()]);
        assert!(s.list_dir("card1/no-such-dir").await.unwrap().is_empty());
    }
}
