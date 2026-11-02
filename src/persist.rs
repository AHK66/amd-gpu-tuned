use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use tokio::sync::mpsc;

use crate::control::Control;
use crate::driver::Gpu;
use crate::store::Store;
use crate::types::PresetSettings;

/// (source, settings) of the last applied state, if any.
pub fn read_active_state(
    store: &Store,
) -> Option<(String, BTreeMap<String, PresetSettings>)> {
    let raw = store.get_meta("active_state").ok().flatten()?;
    let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let source = v.get("source")?.as_str()?.to_string();
    let settings = serde_json::from_value(v.get("settings")?.clone()).ok()?;
    Some((source, settings))
}

/// Revert any GPU whose `active_sources` label points at a preset that no
/// longer exists (a dangling `preset:<id>`). Such a label makes the UI show a
/// stuck "custom" and edits stop persisting to a real preset. Best-effort: a
/// busy control (apply in flight) leaves the label for the next pass.
pub async fn heal_stale_preset_sources(control: &Control, store: &Store) {
    let Some(raw) = store.get_meta("active_sources").ok().flatten() else {
        return;
    };
    let Value::Object(map) = serde_json::from_str::<Value>(&raw).unwrap_or(Value::Null) else {
        return;
    };
    let live: Vec<String> = control.gpus().into_iter().map(|g| g.slot).collect();
    for (slot, src) in &map {
        let Some(n) = src.as_str().and_then(|s| s.strip_prefix("preset:")) else {
            continue;
        };
        let Some(id) = n.parse::<i64>().ok() else {
            continue;
        };
        if !live.iter().any(|s| s == slot) {
            continue;
        }
        if store.get_preset(id).is_ok_and(|p| p.is_some()) {
            continue; // preset still exists — not stale
        }
        match control.revert_for_slot(slot).await {
            Ok(_) => store.set_active_source(slot, "stock"),
            Err(e) => tracing::warn!(slot, "heal: revert of stale preset {id} failed: {e}"),
        }
    }
}

pub async fn reapply(control: &Control, store: &Store, source: &str) {
    let (prev, settings) = match read_active_state(store) {
        Some(t) => t,
        None => return,
    };
    if prev == "stock"
        || settings.is_empty()
        || settings.values().all(|s| s.knob_count() == 0)
    {
        return;
    }
    match control.apply_settings(&settings, source).await {
        Ok(r) => tracing::info!(source, ok = r.ok, "re-applied active state"),
        Err(e) => tracing::warn!(source, "re-apply failed: {e}"),
    }
}

pub async fn boot_reapply(control: Arc<Control>, store: Arc<Store>) {
    heal_stale_preset_sources(&control, &store).await;
    reapply(&control, &store, "boot").await;
}

// ---------- logind ----------

pub async fn handle_prepare_for_sleep(
    control: Arc<Control>,
    store: Arc<Store>,
    mut rx: mpsc::Receiver<bool>,
    resume_settle: Duration,
) {
    while let Some(preparing) = rx.recv().await {
        if !preparing {
            // Spec §11: give the driver/thermals a moment after resume
            // before re-asserting state.
            tokio::time::sleep(resume_settle).await;
            reapply(&control, &store, "resume").await;
        }
    }
}

pub async fn logind_watch(control: Arc<Control>, store: Arc<Store>) {
    use futures_util::StreamExt;
    let (tx, rx) = mpsc::channel::<bool>(4);
    let h = tokio::spawn(handle_prepare_for_sleep(
        control.clone(),
        store.clone(),
        rx,
        Duration::from_secs(2),
    ));
    let res: Result<(), Box<dyn std::error::Error + Send + Sync>> = async {
        // logind (org.freedesktop.login2) lives on the **system** bus — a
        // session-bus connection would compile but never see the signal.
        let conn = zbus::connection::Builder::system()?.build().await?;
        // zbus 5 has no `Connection::recv()`: consume the `MessageStream`
        // (which implements `Stream`).
        let mut stream = zbus::MessageStream::from(&conn);
        while let Some(msg) = stream.next().await {
            let msg = msg?;
            let h = msg.header();
            if h.message_type() != zbus::message::Type::Signal {
                continue;
            }
            if h.path().map(|p| p.as_str()) != Some("/org/freedesktop/login2") {
                continue;
            }
            if h.member().map(|m| m.as_str()) != Some("PrepareForSleep") {
                continue;
            }
            let (preparing,): (bool,) = msg.body().deserialize()?;
            if tx.send(preparing).await.is_err() {
                break;
            }
        }
        Ok(())
    }
    .await;
    if let Err(e) = res {
        tracing::warn!("logind watch unavailable (continuing without sleep/resume re-apply): {e}");
    }
    let _ = h.await;
}

// ---------- drm uevents ----------

#[derive(Debug, Clone)]
pub struct Uevent {
    pub action: String,
    pub subsystem: String,
    pub devpath: String,
}

#[async_trait::async_trait]
pub trait GpuSource: Send + Sync {
    async fn enumerate(&self) -> Vec<Gpu>;
}

pub struct FsGpuSource;

#[async_trait::async_trait]
impl GpuSource for FsGpuSource {
    async fn enumerate(&self) -> Vec<Gpu> {
        // Production enumeration: the Task 7 free function over FsSysfs.
        let fs = crate::driver::sysfs::FsSysfs {
            root: std::path::PathBuf::from("/sys/class/drm"),
        };
        crate::driver::enum_gpus(&fs, 3, Duration::from_millis(500))
            .await
            .unwrap_or_default()
    }
}

const DEBOUNCE: Duration = Duration::from_millis(500);

fn is_drm(e: &Uevent) -> bool {
    e.subsystem == "drm" && (e.action == "add" || e.action == "remove")
}

pub async fn handle_uevents(
    control: Arc<Control>,
    store: Arc<Store>,
    mut rx: mpsc::Receiver<Uevent>,
    source: Arc<dyn GpuSource>,
) {
    loop {
        // Wait for the next relevant (drm add/remove) event, or channel close.
        let first: std::time::Instant;
        loop {
            match rx.recv().await {
                Some(e) if is_drm(&e) => {
                    tracing::info!(action = %e.action, devpath = %e.devpath, "drm uevent");
                    first = std::time::Instant::now();
                    break;
                }
                Some(_) => {}   // other subsystems: ignore
                None => return, // channel closed
            }
        }
        // Debounce: keep extending the deadline while events keep arriving.
        let mut deadline = first + DEBOUNCE;
        loop {
            tokio::select! {
                ev = rx.recv() => {
                    if let Some(e) = ev {
                        if is_drm(&e) {
                            deadline = std::time::Instant::now() + DEBOUNCE;
                        }
                    }
                }
                _ = tokio::time::sleep_until(deadline.into()) => break,
            }
        }
        let gpus = source.enumerate().await;
        tracing::info!(count = gpus.len(), "re-enumerated GPUs");
        control.set_gpus(gpus);
        reapply(&control, &store, "uevent").await;
    }
}

pub fn uevent_watch(control: Arc<Control>, store: Arc<Store>) {
    let (tx, rx) = mpsc::channel::<Uevent>(16);
    let h = tokio::spawn(handle_uevents(
        control.clone(),
        store,
        rx,
        Arc::new(FsGpuSource),
    ));
    tokio::spawn(async move {
        let tx2 = tx.clone();
        if let Err(e) = tokio::task::spawn_blocking(move || netlink_loop(&tx2)).await {
            tracing::warn!("drm uevent watch unavailable: {e}");
        }
        drop(tx); // close the channel so handle_uevents exits
        let _ = h.await;
    });
}

/// Blocking netlink recv, forwarding drm uevents to `tx` (run in spawn_blocking).
/// `nix` has no `SockaddrNl` type, so bind/recv use libc directly.
fn netlink_loop(tx: &mpsc::Sender<Uevent>) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // nix 0.31 has no netlink address-family helper of our shape, so the
    // socket itself comes from libc (errno via nix::Error::last()).
    let fd = unsafe {
        libc::socket(
            libc::AF_NETLINK,
            libc::SOCK_DGRAM | libc::SOCK_CLOEXEC,
            libc::NETLINK_KOBJECT_UEVENT,
        )
    };
    if fd < 0 {
        return Err(nix::Error::last().into());
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct SockaddrNl {
        nl_family: u16,
        nl_protocol: u16,
        nl_pid: u32,
        nl_groups: u32,
    }
    let addr = SockaddrNl {
        nl_family: libc::AF_NETLINK as u16,
        nl_protocol: 0,
        nl_pid: 0,
        nl_groups: 1, // all uevents
    };
    let r = unsafe {
        libc::bind(
            fd,
            &addr as *const SockaddrNl as *const libc::sockaddr,
            std::mem::size_of::<SockaddrNl>() as u32,
        )
    };
    if r < 0 {
        return Err(nix::Error::last().into());
    }
    loop {
        let mut buf = vec![0u8; 8192];
        let mut from: libc::sockaddr = unsafe { std::mem::zeroed() };
        let mut fromlen = std::mem::size_of::<libc::sockaddr>() as u32;
        let n = unsafe {
            libc::recvfrom(
                fd,
                buf.as_mut_ptr() as *mut libc::c_void,
                buf.len(),
                0,
                &mut from,
                &mut fromlen,
            )
        };
        if n < 0 {
            match nix::Error::last() {
                nix::Error::EINTR => continue,
                e => return Err(e.into()),
            }
        }
        buf.truncate(n as usize);
        if let Some(e) = parse_uevent(&buf) {
            if e.subsystem == "drm" && tx.blocking_send(e).is_err() {
                return Ok(()); // handler gone
            }
        }
    }
}

/// Parse a raw netlink `KOBJECT_UEVENT` datagram.
///
/// Wire format: a 16-byte `nlmsghdr` (`nlmsg_len u32, nlmsg_type u16,
/// nlmsg_flags u16, nlmsg_seq u32, nlmsg_pid u32`) followed by **NUL-terminated**
/// `KEY=value` pairs (not newline-separated). Skipping the header and
/// splitting on `\n` (the old behavior) left a binary prefix on the first
/// pair, so `ACTION` never matched and the production re-apply never fired.
fn parse_uevent(buf: &[u8]) -> Option<Uevent> {
    const NLMSG_HDRLEN: usize = 16;
    if buf.len() < NLMSG_HDRLEN {
        return None;
    }
    let payload = &buf[NLMSG_HDRLEN..];
    let mut action = String::new();
    let mut subsystem = String::new();
    let mut devpath = String::new();
    for seg in payload.split(|&b| b == 0) {
        if seg.is_empty() {
            continue;
        }
        let Ok(s) = std::str::from_utf8(seg) else {
            continue;
        };
        let (k, v) = match s.split_once('=') {
            Some(p) => p,
            None => continue,
        };
        match k {
            "ACTION" => action = v.to_string(),
            "SUBSYSTEM" => subsystem = v.to_string(),
            "DEVPATH" => devpath = v.to_string(),
            _ => {}
        }
    }
    if subsystem.is_empty() {
        None
    } else {
        Some(Uevent {
            action,
            subsystem,
            devpath,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::Control;
    use crate::driver::fake;
    use crate::store::Store;
    use crate::types::PresetSettings;
    use std::sync::Arc;

    /// Full fixture-format OD table (current values + ranges), matching what
    /// `od::parse_table` expects (whitespace-separated, Mhz/mv suffixes).
    const OD_TABLE: &str = "OD_SCLK_OFFSET:\n0Mhz\nOD_MCLK:\n0: 200Mhz\n1: 1250Mhz\nOD_VDDGFX_OFFSET:\n0mV\nOD_RANGE:\nSCLK_OFFSET:    -100Mhz       100Mhz\nMCLK:      200Mhz       1250Mhz\nVDDGFX_OFFSET:    -100mv          0mv";

    // Power-cap nodes live under the hwmon dir (fake gpu uses hwmon0);
    // min/max are required — `apply_power_cap` refuses without bounds.
    const FILES: &[(&str, &str)] = &[
        ("card0/device/pp_od_clk_voltage", OD_TABLE),
        ("card0/device/pp_dpm_sclk", "0: 615Mhz\n1: 2400Mhz *\n"),
        ("card0/device/pp_dpm_mclk", "0: 200Mhz\n1: 1250Mhz *\n"),
        ("card0/device/hwmon/hwmon0/power1_cap", "350000000"),
        ("card0/device/hwmon/hwmon0/power1_cap_min", "120000000"),
        ("card0/device/hwmon/hwmon0/power1_cap_max", "350000000"),
        ("card0/device/power_dpm_force_performance_level", "auto"),
    ];

    /// A control whose GPU was "kernel-reset" (fresh FakeSysfs), with the given
    /// settings already recorded in `active_state`.
    async fn seeded(
        settings: BTreeMap<String, PresetSettings>,
        source: &str,
    ) -> (Arc<Control>, Arc<Store>) {
        let store = Arc::new(Store::memory());
        let g = fake::gpu(FILES).await;
        // Control::new already returns Arc<Control> — do not wrap again.
        let control = Control::new(vec![g], store.clone());
        store
            .create_preset("p", "", &PresetSettings::default())
            .unwrap();
        control.apply_settings(&settings, source).await.unwrap();
        (control, store)
    }

    #[tokio::test]
    async fn boot_reapply_restores_after_kernel_reset() {
        let settings = BTreeMap::from([(
            "1002:75b4-1458:242f-0000:0b:00.0".to_string(),
            PresetSettings {
                sclk_offset_mhz: Some(50),
                ..Default::default()
            },
        )]);
        let (control, store) = seeded(settings, "apply").await;
        // Simulate reboot: fresh GPU (OD table back to stock: sclk 0), same store.
        let g2 = fake::gpu(FILES).await;
        control.set_gpus(vec![g2.clone()]);
        boot_reapply(control.clone(), store).await;
        let t = g2.read_od().await.unwrap();
        assert_eq!(t.sclk_offset_mhz, 50);
    }

    #[tokio::test]
    async fn boot_reapply_restores_both_cards() {
        let store = Arc::new(Store::memory());
        let ga = fake::gpu(FILES).await;
        let control = Control::new(vec![ga], store.clone());
        // Apply two single-slot applies (the per-card pattern).
        let sa = BTreeMap::from([(
            "1002:75b4-1458:242f-0000:0b:00.0".to_string(),
            PresetSettings {
                sclk_offset_mhz: Some(50),
                ..Default::default()
            },
        )]);
        control.apply_settings(&sa, "apply").await.unwrap();
        let sb = BTreeMap::from([(
            "1002:75b4-1458:242f-0000:0a:00.0".to_string(),
            PresetSettings {
                power_cap_w: Some(250.0),
                ..Default::default()
            },
        )]);
        control.apply_settings(&sb, "apply").await.unwrap();
        let st = control.active_state().unwrap();
        assert!(
            st["settings"].as_object().unwrap().len() == 2,
            "both slots accumulated"
        );

        // Simulate reboot: fresh GPUs, same store.
        let g2a = fake::gpu(FILES).await;
        let g2b = crate::driver::fake::gpu_with_slot(FILES, "1002:75b4-1458:242f-0000:0a:00.0");
        control.set_gpus(vec![g2a.clone(), g2b.clone()]);
        boot_reapply(control.clone(), store).await;
        assert_eq!(
            g2a.read_od().await.unwrap().sclk_offset_mhz,
            50,
            "card A restored"
        );
        let cap_b = g2b.read_power_cap_w().await.unwrap();
        assert!((cap_b - 250.0).abs() < 1.0, "card B restored");
    }

    #[tokio::test]
    async fn boot_reapply_skips_stock_and_empty() {
        let (control, store) = seeded(BTreeMap::new(), "stock").await;
        // Fresh GPU; boot re-apply must not touch it (source == "stock", 0 knobs).
        let g2 = fake::gpu(FILES).await;
        control.set_gpus(vec![g2.clone()]);
        boot_reapply(control.clone(), store).await;
        assert_eq!(g2.read_od().await.unwrap().sclk_offset_mhz, 0); // OD untouched
        assert_eq!(g2.read_power_cap_w().await.unwrap(), 350.0); // stock cap untouched
    }

    #[tokio::test]
    async fn boot_reapply_per_slot_map_roundtrips() {
        // The map shape must round-trip through active_state, not silently no-op.
        let settings = BTreeMap::from([(
            "1002:75b4-1458:242f-0000:0b:00.0".to_string(),
            PresetSettings {
                sclk_offset_mhz: Some(50),
                power_cap_w: Some(250.0),
                ..Default::default()
            },
        )]);
        let (control, store) = seeded(settings, "apply").await;
        // Simulate reboot: fresh GPU, same store.
        let g2 = fake::gpu(FILES).await;
        control.set_gpus(vec![g2.clone()]);
        boot_reapply(control.clone(), store).await;
        let t = g2.read_od().await.unwrap();
        assert_eq!(t.sclk_offset_mhz, 50, "sclk must be re-applied from the map");
        let cap = g2.read_power_cap_w().await.unwrap();
        assert!((cap - 250.0).abs() < 1.0, "power cap must be re-applied from the map");
    }

    #[tokio::test]
    async fn boot_reapply_empty_map_is_noop() {
        let settings: BTreeMap<String, PresetSettings> = BTreeMap::new();
        let (control, store) = seeded(settings, "apply").await;
        let g2 = fake::gpu(FILES).await;
        control.set_gpus(vec![g2.clone()]);
        boot_reapply(control.clone(), store).await;
        // OD untouched (stock value).
        assert_eq!(g2.read_od().await.unwrap().sclk_offset_mhz, 0);
    }

    struct FakeSource(Vec<Gpu>);
    #[async_trait::async_trait]
    impl GpuSource for FakeSource {
        async fn enumerate(&self) -> Vec<Gpu> {
            self.0.clone()
        }
    }

    #[tokio::test]
    async fn uevent_debounces_and_reapplies() {
        let settings = BTreeMap::from([(
            "1002:75b4-1458:242f-0000:0b:00.0".to_string(),
            PresetSettings {
                power_cap_w: Some(250.0),
                ..Default::default()
            },
        )]);
        let (control, store) = seeded(settings, "apply").await;
        let fresh = fake::gpu(FILES).await;
        let source: Arc<dyn GpuSource> = Arc::new(FakeSource(vec![fresh.clone()]));
        let (tx, rx) = tokio::sync::mpsc::channel(8);
        let h = tokio::spawn(handle_uevents(control.clone(), store, rx, source));
        tx.send(Uevent {
            action: "add".into(),
            subsystem: "drm".into(),
            devpath: "/class/drm/card0".into(),
        })
        .await
        .unwrap();
        tx.send(Uevent {
            action: "add".into(),
            subsystem: "drm".into(),
            devpath: "/class/drm/card1".into(),
        })
        .await
        .unwrap();
        // 2 events within the debounce window -> exactly one re-enumeration/apply.
        tokio::time::sleep(std::time::Duration::from_millis(800)).await;
        assert_eq!(control.gpus().len(), 1);
        // The re-apply wrote the power cap into the fresh GPU.
        let cap = fresh.read_power_cap_w().await.unwrap();
        assert!((cap - 250.0).abs() < 1.0);
        drop(tx);
        h.await.unwrap();
    }

    #[tokio::test]
    async fn uevent_ignores_other_subsystems() {
        // Non-drm events must not trigger re-enumeration: the control keeps
        // its original GPU (set_gpus is only called on debounced drm events).
        let (control, _store) = seeded(BTreeMap::new(), "stock").await;
        let source: Arc<dyn GpuSource> = Arc::new(FakeSource(Vec::new()));
        let (tx, rx) = tokio::sync::mpsc::channel(8);
        let h = tokio::spawn(handle_uevents(control.clone(), _store, rx, source));
        tx.send(Uevent {
            action: "add".into(),
            subsystem: "usb".into(),
            devpath: "/usb1".into(),
        })
        .await
        .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(700)).await;
        assert_eq!(control.gpus().len(), 1); // original GPU untouched
        drop(tx);
        h.await.unwrap();
    }

    #[test]
    fn parse_uevent_netlink_framing() {
        // Real `KOBJECT_UEVENT` datagram: 16-byte `nlmsghdr` + NUL-terminated
        // `KEY=value` pairs. This is the production path — the debounce test
        // above injects `Uevent`s directly and cannot catch framing bugs.
        let mut buf = vec![0u8; 16];
        for kv in ["ACTION=add", "SUBSYSTEM=drm", "DEVPATH=/class/drm/card0"] {
            buf.extend_from_slice(kv.as_bytes());
            buf.push(0);
        }
        let e = parse_uevent(&buf).unwrap();
        assert_eq!(e.action, "add");
        assert_eq!(e.subsystem, "drm");
        assert_eq!(e.devpath, "/class/drm/card0");
        assert!(parse_uevent(&buf[..16]).is_none()); // header only: no payload
    }

    #[tokio::test]
    async fn logind_resume_reapplies() {
        let settings = BTreeMap::from([(
            "1002:75b4-1458:242f-0000:0b:00.0".to_string(),
            PresetSettings {
                power_cap_w: Some(250.0),
                ..Default::default()
            },
        )]);
        let (control, store) = seeded(settings, "apply").await;
        let g2 = fake::gpu(FILES).await;
        control.set_gpus(vec![g2.clone()]);
        let (tx, rx) = tokio::sync::mpsc::channel::<bool>(4);
        let h = tokio::spawn(handle_prepare_for_sleep(
            control.clone(),
            store,
            rx,
            Duration::from_millis(0),
        ));
        tx.send(true).await.unwrap(); // preparing to sleep: no-op
        tx.send(false).await.unwrap(); // resumed: re-apply (0 s settle in tests)
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        let cap = g2.read_power_cap_w().await.unwrap();
        assert!((cap - 250.0).abs() < 1.0);
        drop(tx);
        h.await.unwrap();
    }
}
