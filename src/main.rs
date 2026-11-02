mod api;
mod builtin;
mod config;
mod control;
mod driver;
mod drift;
mod persist;
mod setup;
mod store;
mod telemetry;
mod types;
// Intentionally dormant since 2026-10-18: the watchdog never activates. Its
// runtime path (start -> tick -> trigger) has no production caller, so
// suppress the resulting dead_code.
#[allow(dead_code)]
mod watchdog;
mod web;

use clap::Parser;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "gpu_tuned=info".into()),
        )
        .init();

    let cfg = config::Config::parse();
    std::fs::create_dir_all(&cfg.data)?;

    if let Some(config::Cmd::SetPassword { clear }) = &cfg.cmd {
        return set_password(&cfg.data, *clear);
    }

    // Can this process write the amdgpu sysfs tuning nodes? True only as root
    // (the `sudo` deployment model). Non-root ⇒ every apply/revert/watchdog
    // write is blocked by the OS; the UI surfaces this and locks the watchdog.
    let privileged = unsafe { libc::geteuid() } == 0;
    if !privileged {
        tracing::warn!(
            "running without root — GPU writes are blocked (start with `sudo` to apply tuning)"
        );
    }

    let store = Arc::new(store::Store::open(&cfg.data.join("amd-gpu-tuned.db"))?);
    let ring_min = store::ring_minutes_from_meta(&store);

    // Reconcile the shipped built-in presets (spec §5.2): a parse failure is
    // a build bug and is fatal.
    let builtins = builtin::parse()?;
    let (inserted, updated, removed) = store.upsert_builtins(&builtins.presets)?;
    tracing::info!(
        inserted,
        updated,
        removed,
        "seeded {} built-in presets",
        inserted + updated
    );

    // Enumerate GPUs. `--fake-sysfs` swaps the live sysfs for a captured
    // fixture tree (e2e); default is unchanged: real `/sys` with retries.
    let fake = cfg.fake_sysfs.is_some();
    let gpus = if let Some(fixture) = &cfg.fake_sysfs {
        let gpus = driver::fixture::load_fixture(fixture)?;
        anyhow::ensure!(
            !gpus.is_empty(),
            "no GPUs found in fixture tree {}",
            fixture.display()
        );
        gpus
    } else {
        let fs = driver::sysfs::FsSysfs {
            root: std::path::PathBuf::from("/sys/class/drm"),
        };
        driver::enum_gpus(&fs, 5, Duration::from_secs(3)).await?
    };
    anyhow::ensure!(!gpus.is_empty(), "no AMD GPUs enumerated");
    tracing::info!(
        count = gpus.len(),
        "GPUs enumerated: {:?}",
        gpus.iter().map(|g| g.slot.clone()).collect::<Vec<_>>()
    );

    let control = control::Control::new(gpus, store.clone());
    // First start: capture the stock baseline (spec §6.6).
    if store.get_meta("stock_baseline").ok().flatten().is_none() {
        match control.capture_baseline().await {
            Ok(_) => tracing::info!("stock baseline captured"),
            Err(e) => tracing::warn!("stock baseline capture failed: {e}"),
        }
    }

    if let Err(e) = cfg.validate_intervals() {
        eprintln!("{e}");
        std::process::exit(2);
    }

    let telemetry = telemetry::Telemetry::new(
        control.clone(),
        (ring_min * 60) as u64,
        if fake { None } else { Some("/sys/class/drm".into()) },
    );
    telemetry.start(Duration::from_secs_f64(cfg.poll_s));
    if cfg.record_s > 0.0 {
        telemetry.start_fast(Duration::from_secs_f64(cfg.record_s));
    }

    let watchdog = watchdog::Watchdog::new(
        control.clone(),
        telemetry.clone(),
        store.clone(),
        privileged,
    );
    // Permanently dormant (2026-10-18): the background evaluation loop is not
    // started. `Watchdog` stays in AppState for `GET /api/watchdog` status.

    if !fake {
        persist::boot_reapply(control.clone(), store.clone()).await;
        tokio::spawn(persist::logind_watch(control.clone(), store.clone()));
        persist::uevent_watch(control.clone(), store.clone());
    }

    // Drift guard (spec §5.7): start only after boot_reapply has run, so the
    // (immediate) first tick never logs a spurious episode for the normal
    // boot flow. `pending` is never a final state — close rows left over
    // from a previous run (spec §5.8).
    let drift = drift::DriftGuard::new(control.clone(), store.clone());
    match store.close_stale_drift_pending() {
        Ok(closed) if !closed.is_empty() => {
            tracing::info!(count = closed.len(), "closed stale drift-pending entries from a previous run");
        }
        Err(e) => tracing::warn!("closing stale drift entries failed: {e}"),
        _ => {}
    }
    drift.start();

    let state = api::AppState {
        cfg: Arc::new(cfg.clone()),
        store,
        auth: Arc::new(api::auth::AuthState::new()),
        control,
        drift,
        live_mask: if fake { None } else { setup::read_live_mask() },
        telemetry,
        watchdog,
        privileged,
    };
    let app = api::build_router(state).layer(tower_http::trace::TraceLayer::new_for_http());

    let port = config::resolve_port(cfg.port, builtins.port);
    let addr = SocketAddr::new(IpAddr::from([127, 0, 0, 1]), port);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("amd-gpu-tuned listening on http://127.0.0.1:{}", addr.port());
    axum::serve(listener, app).await?;
    Ok(())
}

/// `amd-gpu-tuned set-password` — stores (or with `clear`, deletes) the
/// write-endpoint password hash in `<data>/amd-gpu-tuned.db`. Safe while the
/// daemon runs: SQLite file locking, and the daemon reads the hash per
/// unlock, so no restart is needed.
fn set_password(data: &std::path::Path, clear: bool) -> anyhow::Result<()> {
    let db = data.join("amd-gpu-tuned.db");
    if clear {
        let store = store::Store::open(&db)?;
        store.delete_meta(api::auth::AUTH_PASSWORD_HASH_KEY);
        anyhow::ensure!(
            store.get_meta(api::auth::AUTH_PASSWORD_HASH_KEY)?.is_none(),
            "could not verify the password hash was cleared — is the amd-gpu-tuned daemon holding the database?"
        );
        println!("password hash cleared (open mode) — db {}", db.display());
        return Ok(());
    }
    let pw1 = rpassword::prompt_password("new password: ")?;
    let pw2 = rpassword::prompt_password("confirm password: ")?;
    anyhow::ensure!(pw1.len() >= 8, "password must be at least 8 characters");
    anyhow::ensure!(pw1 == pw2, "passwords do not match");
    let store = store::Store::open(&db)?;
    let hash = api::auth::hash_password(&pw1);
    store.set_meta(api::auth::AUTH_PASSWORD_HASH_KEY, &hash);
    anyhow::ensure!(
        store.get_meta(api::auth::AUTH_PASSWORD_HASH_KEY)?.as_deref() == Some(hash.as_str()),
        "could not verify the stored hash — is the amd-gpu-tuned daemon holding the database?"
    );
    println!("password hash stored (auth enabled) — db {}", db.display());
    Ok(())
}
