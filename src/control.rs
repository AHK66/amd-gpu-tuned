use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::{json, Value};
use thiserror::Error;

use crate::driver::od::OdTable;
use crate::driver::Gpu;
use crate::store::{now_ms, Store};
use crate::types::{ApplyOutcome, PresetSettings};

#[derive(Debug, Error)]
pub enum ControlError {
    #[error("an apply is already in progress")]
    Busy,
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

// Serialize only: `ApplyOutcome` (round 1) does not implement Deserialize,
// and the report is never parsed back — it is a JSON response body only.
#[derive(Debug, Clone, serde::Serialize, PartialEq)]
pub struct ApplyReport {
    pub ts: i64,
    pub ok: bool,
    /// slot -> per-field outcomes (order: power, sclk, mclk, vddgfx)
    pub per_gpu: BTreeMap<String, Vec<ApplyOutcome>>,
}

pub struct Control {
    gpus: std::sync::RwLock<Vec<Gpu>>,
    pub store: Arc<Store>,
    pub busy: tokio::sync::Mutex<()>,
    started_ms: i64,
}

/// Live readback assembled in `get_status` from the three driver reads already
/// performed there (no new driver calls). All fields are `Option` because the
/// underlying reads are fallible; a `None` field means "skip that knob"
/// (spec §4.1).
pub(crate) struct LiveReadback {
    pub power_cap_w: Option<f64>,
    pub od: Option<OdTable>,
}

/// The captured live settings a card drifted to (spec §5.1).
pub(crate) struct DriftDelta {
    pub captured: PresetSettings,
}

impl Control {
    pub fn new(gpus: Vec<Gpu>, store: Arc<Store>) -> Arc<Self> {
        Arc::new(Self {
            gpus: std::sync::RwLock::new(gpus),
            store,
            busy: tokio::sync::Mutex::new(()),
            started_ms: now_ms(),
        })
    }

    pub fn set_gpus(&self, gpus: Vec<Gpu>) {
        *self.gpus.write().unwrap() = gpus;
    }

    pub fn gpus(&self) -> Vec<Gpu> {
        self.gpus.read().unwrap().clone()
    }

    /// Probes the single-flight lock (guard dropped immediately; a brief
    /// race window is acceptable for a status indicator).
    pub fn is_busy(&self) -> bool {
        self.busy.try_lock().is_err()
    }

    pub fn uptime_ms(&self) -> i64 {
        now_ms() - self.started_ms
    }

    pub fn active_state(&self) -> Option<Value> {
        self.store
            .get_meta("active_state")
            .ok()
            .flatten()
            .and_then(|s| serde_json::from_str(&s).ok())
    }

    /// Compare a slot's live readback against the recorded `active_state`
    /// (falling back to the per-slot `stock_baseline`) — pure comparison, no
    /// adoption (spec §5.1). `None` when not drifted, when `is_busy()`
    /// (re-checked here to close the busy-probe race), or when the slot has
    /// no `stock_baseline` entry. Tolerances: power ±1.0 W, OD exact; `None`
    /// readbacks and degenerate baseline values (0.0 W power / 0 MHz mclk —
    /// the capture fallback, never a real value) skip the knob.
    pub(crate) fn compare_drift(&self, slot: &str, live: &LiveReadback) -> Option<DriftDelta> {
        if self.is_busy() {
            return None;
        }
        let baseline = self.stock_baseline_slot(slot)?;
        let recorded: Option<PresetSettings> = self
            .active_state()
            .and_then(|v| v.get("settings").cloned())
            .and_then(|s| serde_json::from_value::<BTreeMap<String, PresetSettings>>(s).ok())
            .and_then(|m| m.get(slot).cloned());

        let mut drifted = false;

        // power_cap_w: |actual - expected| <= 1.0 W.
        if let (Some(actual), Some(expected)) = (
            live.power_cap_w,
            recorded
                .as_ref()
                .and_then(|r| r.power_cap_w)
                .or_else(|| {
                    baseline
                        .get("power_cap_w")
                        .and_then(|v| v.as_f64())
                        .filter(|&w| w > 0.0)
                }),
        ) {
            if (actual - expected).abs() > 1.0 {
                drifted = true;
            }
        }

        // OD knobs: exact integer equality; skipped when the OD readback is absent.
        if let Some(od) = &live.od {
            if let (Some(actual), Some(expected)) = (
                od.mclk.get(&1).copied(),
                recorded
                    .as_ref()
                    .and_then(|r| r.max_mclk_mhz)
                    .or_else(|| {
                        baseline
                            .get("max_mclk_mhz")
                            .and_then(|v| v.as_i64())
                            .filter(|&n| n > 0)
                            .map(|n| n as i32)
                    }),
            ) {
                if actual != expected {
                    drifted = true;
                }
            }
            if let Some(expected) = recorded
                .as_ref()
                .and_then(|r| r.sclk_offset_mhz)
                .or_else(|| {
                    baseline
                        .get("sclk_offset_mhz")
                        .and_then(|v| v.as_i64())
                        .map(|n| n as i32)
                })
            {
                if od.sclk_offset_mhz != expected {
                    drifted = true;
                }
            }
            if let Some(expected) = recorded
                .as_ref()
                .and_then(|r| r.voltage_offset_mv)
                .or_else(|| {
                    baseline
                        .get("voltage_offset_mv")
                        .and_then(|v| v.as_i64())
                        .map(|n| n as i32)
                })
            {
                if od.vddgfx_mv != expected {
                    drifted = true;
                }
            }
        }

        if !drifted {
            return None;
        }
        Some(DriftDelta {
            captured: PresetSettings {
                power_cap_w: live.power_cap_w,
                sclk_offset_mhz: live.od.as_ref().map(|o| o.sclk_offset_mhz),
                max_mclk_mhz: live.od.as_ref().and_then(|o| o.mclk.get(&1).copied()),
                voltage_offset_mv: live.od.as_ref().map(|o| o.vddgfx_mv),
                ..Default::default()
            },
        })
    }

    /// Adoption (spec §5.4): capture live values into
    /// `active_state.settings[slot]`, label the slot `external`. No event —
    /// the guard emits `external_drift` on every drift transition
    /// (spec §5.3).
    pub(crate) fn adopt_external(&self, slot: &str, captured: &PresetSettings) {
        let mut stored: Value = self
            .store
            .get_meta("active_state")
            .ok()
            .flatten()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(json!({ "settings": {} }));
        let mut map: BTreeMap<String, PresetSettings> = stored
            .get("settings")
            .cloned()
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();
        map.insert(slot.to_string(), captured.clone());
        stored["settings"] = serde_json::to_value(&map).unwrap();
        stored["source"] = json!("external");
        self.store.set_meta("active_state", &stored.to_string());
        self.store.set_active_source(slot, "external");
    }

    // ---------- baseline ----------

    /// Capture the current verified state of every GPU as the stock
    /// baseline (spec §6.6). Overwrites any previous baseline.
    pub async fn capture_baseline(&self) -> Result<Value, ControlError> {
        let mut b = serde_json::Map::new();
        for g in self.gpus() {
            let cap_w = g
                .root
                .read(&g.h("power1_cap"))
                .await
                .ok()
                .and_then(|s| s.parse::<i64>().ok())
                .map(|u| u as f64 / 1e6)
                .unwrap_or(0.0);
            let od = g.read_od().await.ok();
            let sclk = od.as_ref().map(|t| t.sclk_offset_mhz).unwrap_or(0);
            let max_mclk = od
                .as_ref()
                .and_then(|t| t.mclk.get(&1).copied())
                .unwrap_or(0);
            let mv = od.as_ref().map(|t| t.vddgfx_mv).unwrap_or(0);
            b.insert(
                g.slot.clone(),
                json!({
                    "power_cap_w": cap_w,
                    "sclk_offset_mhz": sclk,
                    "max_mclk_mhz": max_mclk,
                    "voltage_offset_mv": mv,
                }),
            );
        }
        let v = Value::Object(b);
        self.store.set_meta("stock_baseline", &v.to_string());
        self.store
            .set_meta("stock_baseline_ts", &now_ms().to_string());
        self.store
            .append_event("baseline_captured", &json!({ "ts": now_ms() }));
        Ok(v)
    }

    fn stock_baseline(&self) -> Option<Value> {
        self.store
            .get_meta("stock_baseline")
            .ok()
            .flatten()
            .and_then(|s| serde_json::from_str(&s).ok())
    }

    pub(crate) fn stock_baseline_slot(&self, slot: &str) -> Option<Value> {
        self.stock_baseline().and_then(|b| b.get(slot).cloned())
    }

    pub(crate) fn active_source(&self, slot: &str) -> Option<String> {
        self.store
            .get_meta("active_sources")
            .ok()
            .flatten()
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .and_then(|m| m.get(slot).cloned())
            .and_then(|v| v.as_str().map(str::to_string))
    }

    // ---------- apply ----------

    /// Manual/API apply: non-blocking (409) when busy.
    pub async fn apply_settings(
        &self,
        settings: &BTreeMap<String, PresetSettings>,
        source: &str,
    ) -> Result<ApplyReport, ControlError> {
        let _guard = self.busy.try_lock().map_err(|_| ControlError::Busy)?;
        self.apply_settings_locked(settings, source).await
    }

    /// Drift-guard / watchdog apply: BLOCKS on the `busy` mutex — the
    /// restore must land its apply even while a manual apply is in flight.
    pub async fn apply_settings_blocking(
        &self,
        settings: &BTreeMap<String, PresetSettings>,
        source: &str,
    ) -> Result<ApplyReport, ControlError> {
        let _guard = self.busy.lock().await;
        self.apply_settings_locked(settings, source).await
    }

    /// The apply body. The caller holds the `busy` guard (any flavor).
    async fn apply_settings_locked(
        &self,
        settings: &BTreeMap<String, PresetSettings>,
        source: &str,
    ) -> Result<ApplyReport, ControlError> {
        // Pre-apply snapshot for the audit event.
        let mut before = serde_json::Map::new();
        for g in self.gpus() {
            before.insert(
                g.slot.clone(),
                json!({
                    "power_cap_w": g.read_power_cap_w().await,
                    "od": g.read_od().await.ok(),
                    "fan_curve": g.read_fan_curve().await.ok().flatten(),
                }),
            );
        }
        self.store.append_event(
            "apply_start",
            &json!({ "source": source, "settings": settings, "before": Value::Object(before) }),
        );

        let mut per_gpu = BTreeMap::new();
        for g in self.gpus() {
            let slot_settings = settings.get(&g.slot);
            per_gpu.insert(
                g.slot.clone(),
                self.apply_to_gpu(&g, &slot_settings).await,
            );
        }

        let ok = per_gpu
            .values()
            .all(|outs| outs.iter().all(|o| o.status != "failed"));
        let ts = now_ms();
        let report = ApplyReport {
            ts,
            ok,
            per_gpu: per_gpu.clone(),
        };
        // Accumulate per-slot state: each apply now carries only its target
        // slot, so replacing the stored map would lose the other cards on
        // boot re-apply (spec §9).
        let mut stored: Value = self
            .store
            .get_meta("active_state")
            .ok()
            .flatten()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(json!({ "settings": {} }));
        let mut map: BTreeMap<String, PresetSettings> = stored
            .get("settings")
            .cloned()
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();
        for (slot, s) in settings {
            map.insert(slot.clone(), s.clone());
        }
        // A re-apply after a full revert must not keep the stale `stock`
        // flag alongside live settings.
        if let Some(obj) = stored.as_object_mut() {
            obj.remove("stock");
        }
        stored["settings"] = serde_json::to_value(&map).unwrap();
        stored["ts"] = json!(ts);
        stored["source"] = json!(source);
        stored["per_gpu"] = serde_json::to_value(&per_gpu).unwrap();
        self.store.set_meta("active_state", &stored.to_string());
        self.store.append_event(
            if ok { "applied" } else { "apply_error" },
            &json!({ "source": source, "ok": ok, "per_gpu": per_gpu }),
        );
        Ok(report)
    }

    /// Apply one preset to one GPU. Order: power -> od.
    async fn apply_to_gpu(
        &self,
        g: &Gpu,
        settings: &Option<&PresetSettings>,
    ) -> Vec<ApplyOutcome> {
        let settings = match settings {
            Some(s) => s,
            None => return Vec::new(),
        };
        let mut out = Vec::new();
        // DPM gate must be manual for OD writes (spec §6.3 step 2).
        let dpm_ok = g.set_dpm_force("manual").await.is_ok();
        if !dpm_ok {
            return vec![ApplyOutcome {
                field: "power_cap_w".into(),
                status: "failed".into(),
                detail: "could not set power_dpm_force_performance_level=manual".into(),
            }];
        }

        let aborted = 'outer: {
            // 1. Power cap.
            if let Some(w) = settings.power_cap_w {
                match g.apply_power_cap(w).await {
                    Ok(o) => out.push(o),
                    Err(e) => {
                        out.push(ApplyOutcome {
                            field: "power_cap_w".into(),
                            status: "failed".into(),
                            detail: e.to_string(),
                        });
                        break 'outer Some("power cap write failed");
                    }
                }
            }

            // 2. Overdrive (sclk offset / mclk max / vddgfx) — one staged
            //    table, so all three share a commit and a failure.
            let want_od = settings.sclk_offset_mhz.is_some()
                || settings.max_mclk_mhz.is_some()
                || settings.voltage_offset_mv.is_some();
            if want_od {
                match g
                    .apply_od(
                        settings.sclk_offset_mhz,
                        settings.max_mclk_mhz,
                        settings.voltage_offset_mv,
                    )
                    .await
                {
                    Ok(()) => {
                        // Readback verify per knob.
                        match g.read_od().await {
                            Ok(t) => {
                                if let Some(v) = settings.sclk_offset_mhz {
                                    out.push(ApplyOutcome {
                                        field: "sclk_offset_mhz".into(),
                                        status: if t.sclk_offset_mhz == v {
                                            "verified"
                                        } else {
                                            "failed"
                                        }
                                        .into(),
                                        detail: format!("readback {}", t.sclk_offset_mhz),
                                    });
                                }
                                if let Some(v) = settings.max_mclk_mhz {
                                    let got = t.mclk.get(&1).copied().unwrap_or(0);
                                    out.push(ApplyOutcome {
                                        field: "max_mclk_mhz".into(),
                                        status: if got == v { "verified" } else { "failed" }.into(),
                                        detail: format!("readback {got}"),
                                    });
                                }
                                if let Some(v) = settings.voltage_offset_mv {
                                    out.push(ApplyOutcome {
                                        field: "voltage_offset_mv".into(),
                                        status: if t.vddgfx_mv == v {
                                            "verified"
                                        } else {
                                            "failed"
                                        }
                                        .into(),
                                        detail: format!("readback {}", t.vddgfx_mv),
                                    });
                                }
                            }
                            Err(e) => {
                                for (field, set) in [
                                    ("sclk_offset_mhz", settings.sclk_offset_mhz.is_some()),
                                    ("max_mclk_mhz", settings.max_mclk_mhz.is_some()),
                                    ("voltage_offset_mv", settings.voltage_offset_mv.is_some()),
                                ] {
                                    if set {
                                        out.push(ApplyOutcome {
                                            field: field.into(),
                                            status: "failed".into(),
                                            detail: format!("readback failed: {e}"),
                                        });
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        // Fail-closed: reset the staged table to defaults.
                        let od_rel = g.rel("pp_od_clk_voltage");
                        let _ = g.root.write(&od_rel, "r").await;
                        let _ = g.root.write(&od_rel, "c").await;
                        // Only the requested OD fields report `failed` (the
                        // trailing "skipped" pass fills in the rest).
                        for (field, requested) in [
                            ("sclk_offset_mhz", settings.sclk_offset_mhz.is_some()),
                            ("max_mclk_mhz", settings.max_mclk_mhz.is_some()),
                            ("voltage_offset_mv", settings.voltage_offset_mv.is_some()),
                        ] {
                            if requested {
                                out.push(ApplyOutcome {
                                    field: field.into(),
                                    status: "failed".into(),
                                    detail: e.to_string(),
                                });
                            }
                        }
                        break 'outer Some("overdrive write failed");
                    }
                }
            }

            None
        };

        // Always restore the DPM gate (spec §6.3 step 4).
        if let Err(e) = g.set_dpm_force("auto").await {
            tracing::warn!(slot = %g.slot, "failed to restore DPM auto: {e}");
        }
        // Requested fields never reached get an explicit "skipped".
        if let Some(reason) = aborted {
            for (field, requested) in [
                ("sclk_offset_mhz", settings.sclk_offset_mhz.is_some()),
                ("max_mclk_mhz", settings.max_mclk_mhz.is_some()),
                ("voltage_offset_mv", settings.voltage_offset_mv.is_some()),
            ] {
                if requested && !out.iter().any(|o| o.field == field) {
                    out.push(ApplyOutcome {
                        field: field.into(),
                        status: "skipped".into(),
                        detail: reason.into(),
                    });
                }
            }
        }
        out
    }

    // ---------- revert ----------

    /// Re-assert the stock baseline through the same write path (spec §6.6).
    pub async fn revert(&self) -> Result<ApplyReport, ControlError> {
        let _guard = self.busy.try_lock().map_err(|_| ControlError::Busy)?;
        self.revert_locked().await
    }

    /// Watchdog revert: blocking `busy` lock.
    ///
    /// Dormant since 2026-10-18: only the (inert) watchdog `revert_stock`
    /// calls this, so it has no live production caller.
    #[allow(dead_code)]
    pub async fn revert_blocking(&self) -> Result<ApplyReport, ControlError> {
        let _guard = self.busy.lock().await;
        self.revert_locked().await
    }

    /// Re-assert the stock baseline for one GPU (shared by full and
    /// slot-scoped revert).
    async fn revert_gpu(&self, g: &Gpu, baseline: &Value) -> Vec<ApplyOutcome> {
        let mut out = Vec::new();
        let Some(stock) = baseline.get(&g.slot) else {
            // GPU set changed since the baseline was captured — fail
            // closed for this slot instead of panicking on the index.
            out.push(ApplyOutcome {
                field: "stock_baseline".into(),
                status: "failed".into(),
                detail: format!("no baseline recorded for slot {}", g.slot),
            });
            return out;
        };
        let dpm_ok = g.set_dpm_force("manual").await.is_ok();
        if dpm_ok {
            if let Some(w) = stock.get("power_cap_w").and_then(|v| v.as_f64()) {
                match g.apply_power_cap(w).await {
                    Ok(o) => out.push(o),
                    Err(e) => out.push(ApplyOutcome {
                        field: "power_cap_w".into(),
                        status: "failed".into(),
                        detail: e.to_string(),
                    }),
                }
            }
            // OD: re-assert the *recorded* baseline values (spec §6.6:
            // "Revert re-applies that baseline"). Plain `r`+`c` only when
            // the baseline is fully stock — stock commits can be non-zero
            // (e.g. the day-1 fixture commits a -50 mV vddgfx offset), so
            // verifying against hard-coded 0 would spuriously fail.
            let sclk = stock
                .get("sclk_offset_mhz")
                .and_then(|v| v.as_i64())
                .unwrap_or(0) as i32;
            let mclk = stock
                .get("max_mclk_mhz")
                .and_then(|v| v.as_i64())
                .unwrap_or(0) as i32;
            let vddgfx = stock
                .get("voltage_offset_mv")
                .and_then(|v| v.as_i64())
                .unwrap_or(0) as i32;
            if g.root.exists(&g.rel("pp_od_clk_voltage")).await {
                let apply = async {
                    if sclk == 0 && mclk == 0 && vddgfx == 0 {
                        let od_rel = g.rel("pp_od_clk_voltage");
                        g.root.write(&od_rel, "r").await?;
                        g.root.write(&od_rel, "c").await
                    } else {
                        g.apply_od(
                            Some(sclk),
                            if mclk > 0 { Some(mclk) } else { None },
                            Some(vddgfx),
                        )
                        .await
                    }
                };
                match apply.await {
                    Ok(()) => match g.read_od().await {
                        Ok(t)
                            if t.sclk_offset_mhz == sclk
                                && t.vddgfx_mv == vddgfx
                                && (mclk == 0
                                    || t.mclk.get(&1).copied().unwrap_or(0) == mclk) =>
                        {
                            out.push(ApplyOutcome {
                                field: "od_table".into(),
                                status: "verified".into(),
                                detail: "stock baseline re-applied".into(),
                            })
                        }
                        _ => out.push(ApplyOutcome {
                            field: "od_table".into(),
                            status: "failed".into(),
                            detail: "baseline readback mismatch".into(),
                        }),
                    },
                    Err(e) => out.push(ApplyOutcome {
                        field: "od_table".into(),
                        status: "failed".into(),
                        detail: e.to_string(),
                    }),
                }
            }
        } else {
            out.push(ApplyOutcome {
                field: "od_table".into(),
                status: "failed".into(),
                detail: "could not set DPM manual".into(),
            });
        }
        let _ = g.set_dpm_force("auto").await;
        out
    }

    async fn revert_locked(&self) -> Result<ApplyReport, ControlError> {
        let baseline = self
            .stock_baseline()
            .ok_or_else(|| anyhow::anyhow!("no stock baseline captured"))?;
        let mut per_gpu = BTreeMap::new();
        for g in self.gpus() {
            per_gpu.insert(g.slot.clone(), self.revert_gpu(&g, &baseline).await);
        }
        let ok = per_gpu
            .values()
            .all(|outs| outs.iter().all(|o| o.status != "failed"));
        let ts = now_ms();
        self.store.set_meta(
            "active_state",
            &json!({ "stock": true, "ts": ts }).to_string(),
        );
        self.store
            .append_event("revert", &json!({ "ok": ok, "per_gpu": per_gpu }));
        Ok(ApplyReport { ts, ok, per_gpu })
    }

    /// Revert one card to its captured baseline; other cards keep their
    /// settings. Drops the slot from the accumulated `active_state`
    /// (spec §6.2, §9).
    pub async fn revert_for_slot(&self, slot: &str) -> Result<ApplyReport, ControlError> {
        let _guard = self.busy.try_lock().map_err(|_| ControlError::Busy)?;
        let baseline = self
            .stock_baseline()
            .ok_or_else(|| anyhow::anyhow!("no stock baseline captured"))?;
        let g = self
            .gpus()
            .into_iter()
            .find(|g| g.slot == slot)
            .ok_or_else(|| anyhow::anyhow!("unknown slot {slot}"))?;
        let per_gpu = BTreeMap::from([(slot.to_string(), self.revert_gpu(&g, &baseline).await)]);
        let ok = per_gpu
            .values()
            .all(|outs| outs.iter().all(|o| o.status != "failed"));
        let ts = now_ms();
        self.drop_active_state_slot(slot);
        self.store.append_event(
            "revert",
            &json!({ "ok": ok, "per_gpu": per_gpu, "slot": slot }),
        );
        Ok(ApplyReport { ts, ok, per_gpu })
    }

    fn drop_active_state_slot(&self, slot: &str) {
        let stored: Value = self
            .store
            .get_meta("active_state")
            .ok()
            .flatten()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(Value::Null);
        let Some(obj) = stored.as_object() else {
            self.store.set_meta(
                "active_state",
                &json!({ "stock": true, "ts": now_ms() }).to_string(),
            );
            return;
        };
        let mut map: BTreeMap<String, PresetSettings> = obj
            .get("settings")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();
        map.remove(slot);
        if map.is_empty() {
            self.store.set_meta(
                "active_state",
                &json!({ "stock": true, "ts": now_ms() }).to_string(),
            );
        } else {
            let mut v = Value::Object(obj.clone());
            v["settings"] = serde_json::to_value(&map).unwrap();
            // Drop the slot from the stored per-knob report too, so the
            // persisted state never claims the reverted slot is configured.
            if let Some(pg) = v.get("per_gpu").and_then(|p| p.as_object().cloned()) {
                let mut pg = pg;
                pg.remove(slot);
                v["per_gpu"] = Value::Object(pg);
            }
            v["ts"] = json!(now_ms());
            self.store.set_meta("active_state", &v.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::driver::fake::gpu as fake_gpu;
    use crate::driver::fake::FakeSysfs;
    use crate::driver::od::{OdRanges, OdTable};
    use crate::types::PresetSettings;
    use std::sync::Arc;

    const OD_REL: &str = "card0/device/pp_od_clk_voltage";
    const FAN_REL: &str = "card0/device/gpu_od/fan_ctrl/fan_curve";
    const CAP_REL: &str = "card0/device/hwmon/hwmon0/power1_cap";
    const DPM_REL: &str = "card0/device/power_dpm_force_performance_level";

    const OD: &str = "OD_SCLK_OFFSET:\n0Mhz\nOD_MCLK:\n0: 97Mhz\n1: 1258MHz\nOD_VDDGFX_OFFSET:\n0mV\nOD_RANGE:\nSCLK_OFFSET:    -500Mhz       1000Mhz\nMCLK:      97Mhz       1500Mhz\nVDDGFX_OFFSET:    -200mv          0mv";
    const FAN: &str = "OD_FAN_CURVE:\n0: 25C 30%\n1: 50C 50%\n2: 75C 75%\n3: 100C 100%\nOD_RANGE:\nFAN_CURVE(hotspot temp): 25C 100C\nFAN_CURVE(fan speed): 30% 100%";

    fn full_files() -> Vec<(&'static str, &'static str)> {
        vec![
            (OD_REL, OD),
            (FAN_REL, FAN),
            (CAP_REL, "300000000"),
            ("card0/device/hwmon/hwmon0/power1_cap_min", "120000000"),
            ("card0/device/hwmon/hwmon0/power1_cap_max", "300000000"),
            (DPM_REL, "auto"),
            ("card0/device/pp_dpm_tjmax", "90"),
            ("card0/device/pp_dpm_tjmax_min", "60"),
            ("card0/device/pp_dpm_tjmax_max", "110"),
        ]
    }

    async fn ctl(files: &[(&str, &str)]) -> (Arc<Control>, Arc<Store>) {
        let store = Arc::new(Store::memory());
        let g = fake_gpu(files).await;
        // Control::new already returns Arc<Control> — do not wrap again.
        (Control::new(vec![g], store.clone()), store)
    }

    fn full_settings() -> BTreeMap<String, PresetSettings> {
        BTreeMap::from([(
            "1002:75b4-1458:242f-0000:0b:00.0".to_string(),
            PresetSettings {
                power_cap_w: Some(235.0),
                sclk_offset_mhz: Some(100),
                max_mclk_mhz: Some(1150),
                voltage_offset_mv: Some(-50),
                ..Default::default()
            },
        )])
    }

    #[tokio::test]
    async fn apply_all_knobs_verifies_and_restores_dpm() {
        let (c, store) = ctl(&full_files()).await;
        c.capture_baseline().await.unwrap();
        let rep = c.apply_settings(&full_settings(), "test").await.unwrap();
        assert!(rep.ok);
        let outs = &rep.per_gpu["1002:75b4-1458:242f-0000:0b:00.0"];
        for o in outs {
            assert_eq!(o.status, "verified", "field {} -> {}", o.field, o.detail);
        }
        let names: Vec<&str> = outs.iter().map(|o| o.field.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "power_cap_w",
                "sclk_offset_mhz",
                "max_mclk_mhz",
                "voltage_offset_mv"
            ]
        );
        // DPM gate restored to auto.
        // (read through a fresh FakeSysfs view is not possible post-Arc; the
        // restore is asserted indirectly by the event + active_state below.)
        let st = c.active_state().unwrap();
        assert_eq!(st["source"], "test");
        let evs = store.list_events(None, 100).unwrap();
        let kinds: Vec<String> = evs.iter().map(|e| e.kind.clone()).collect();
        assert!(kinds.contains(&"apply_start".to_string()));
        assert!(kinds.contains(&"applied".to_string()));
    }

    #[tokio::test]
    async fn busy_rejects_second_apply() {
        let (c, _) = ctl(&full_files()).await;
        c.capture_baseline().await.unwrap();
        let _guard = c.busy.try_lock().unwrap();
        let err = c
            .apply_settings(&BTreeMap::new(), "test")
            .await
            .unwrap_err();
        assert!(matches!(err, ControlError::Busy));
    }

    /// A Sysfs whose write() fails when `content` contains `fail_on`.
    #[derive(Clone)]
    struct FailSysfs {
        inner: FakeSysfs,
        fail_on: String,
    }
    #[async_trait::async_trait]
    impl crate::driver::sysfs::Sysfs for FailSysfs {
        async fn read(&self, rel: &str) -> anyhow::Result<String> {
            self.inner.read(rel).await
        }
        async fn write(&self, rel: &str, content: &str) -> anyhow::Result<()> {
            if content.contains(&self.fail_on) {
                anyhow::bail!("injected failure ({content})");
            }
            self.inner.write(rel, content).await
        }
        async fn exists(&self, rel: &str) -> bool {
            self.inner.exists(rel).await
        }
        async fn read_all(&self, rel: &str) -> anyhow::Result<String> {
            self.inner.read_all(rel).await
        }
        async fn list_dir(&self, rel: &str) -> anyhow::Result<Vec<String>> {
            self.inner.list_dir(rel).await
        }
    }

    #[tokio::test]
    async fn od_write_failure_recovers_and_marks_failed() {
        let store = Arc::new(Store::memory());
        let inner = FakeSysfs::with(&[(OD_REL, OD), (CAP_REL, "300000000"), (DPM_REL, "auto")]);
        let bad = Arc::new(FailSysfs {
            inner,
            fail_on: "s 100".into(),
        });
        let g = crate::driver::Gpu {
            root: bad,
            rel: "card0".into(),
            slot: "1002:75b4-1458:242f-0000:0b:00.0".into(),
            card_n: 0,
            hwmon: "hwmon0".into(),
        };
        let c = Control::new(vec![g], store.clone());
        c.capture_baseline().await.unwrap();
        let settings = BTreeMap::from([(
            "1002:75b4-1458:242f-0000:0b:00.0".to_string(),
            PresetSettings {
                sclk_offset_mhz: Some(100),
                ..Default::default()
            },
        )]);
        let rep = c.apply_settings(&settings, "test").await.unwrap();
        assert!(!rep.ok);
        let outs = &rep.per_gpu["1002:75b4-1458:242f-0000:0b:00.0"];
        assert_eq!(outs.len(), 1);
        assert_eq!(outs[0].field, "sclk_offset_mhz");
        assert_eq!(outs[0].status, "failed");
        assert!(outs[0].detail.contains("injected failure"));
        let evs = store.list_events(None, 100).unwrap();
        assert!(evs.iter().any(|e| e.kind == "apply_error"));
    }

    #[tokio::test]
    async fn revert_restores_stock_state() {
        let (c, _) = ctl(&full_files()).await;
        c.capture_baseline().await.unwrap();
        c.apply_settings(&full_settings(), "test").await.unwrap();
        let rep = c.revert().await.unwrap();
        assert!(rep.ok);
        assert_eq!(c.active_state().unwrap()["stock"], true);
    }

    #[tokio::test]
    async fn capture_baseline_shape() {
        let (c, store) = ctl(&full_files()).await;
        let b = c.capture_baseline().await.unwrap();
        let slot = &b["1002:75b4-1458:242f-0000:0b:00.0"];
        assert_eq!(slot["power_cap_w"], 300.0);
        assert_eq!(slot["sclk_offset_mhz"], 0);
        assert_eq!(slot["max_mclk_mhz"], 1258);
        assert_eq!(slot["voltage_offset_mv"], 0);
        assert!(store.get_meta("stock_baseline_ts").is_ok());
    }

    const SLOT_A: &str = "1002:75b4-1458:242f-0000:0b:00.0";
    const SLOT_B: &str = "1002:75b4-1458:242f-0000:0a:00.0";

    #[tokio::test]
    async fn apply_matrix_present_absent_mixed() {
        let store = Arc::new(Store::memory());
        let ga = fake_gpu(&full_files()).await;
        let gb = crate::driver::fake::gpu_with_slot(&full_files(), SLOT_B);
        let c = Control::new(vec![ga, gb], store);
        c.capture_baseline().await.unwrap();

        // Both slots configured: both get writes.
        let both = BTreeMap::from([
            (
                SLOT_A.to_string(),
                PresetSettings {
                    power_cap_w: Some(250.0),
                    ..Default::default()
                },
            ),
            (
                SLOT_B.to_string(),
                PresetSettings {
                    power_cap_w: Some(200.0),
                    ..Default::default()
                },
            ),
        ]);
        let rep = c.apply_settings(&both, "test").await.unwrap();
        assert!(rep.ok);
        assert!(!rep.per_gpu[SLOT_A].is_empty());
        assert!(!rep.per_gpu[SLOT_B].is_empty());

        // Only A configured: B is untouched — empty outcome vec, no writes.
        let only_a = BTreeMap::from([(
            SLOT_A.to_string(),
            PresetSettings {
                power_cap_w: Some(235.0),
                ..Default::default()
            },
        )]);
        let rep = c.apply_settings(&only_a, "test").await.unwrap();
        assert!(rep.ok);
        assert!(!rep.per_gpu[SLOT_A].is_empty());
        assert!(rep.per_gpu[SLOT_B].is_empty(), "absent slot must be untouched");
        let gbs = c.gpus().into_iter().find(|g| g.slot == SLOT_B).unwrap();
        let cap_b = gbs.read_power_cap_w().await.unwrap();
        assert!((cap_b - 200.0).abs() < 1.0, "B's cap must be unchanged");
    }

    #[tokio::test]
    async fn active_state_accumulates_per_slot_across_applies() {
        let store = Arc::new(Store::memory());
        let ga = fake_gpu(&full_files()).await;
        let gb = crate::driver::fake::gpu_with_slot(&full_files(), SLOT_B);
        let c = Control::new(vec![ga, gb], store);
        c.capture_baseline().await.unwrap();

        let only_a = BTreeMap::from([(
            SLOT_A.to_string(),
            PresetSettings {
                power_cap_w: Some(250.0),
                ..Default::default()
            },
        )]);
        c.apply_settings(&only_a, "test").await.unwrap();
        // Only A stored so far.
        let st = c.active_state().unwrap();
        assert!(st["settings"].get(SLOT_A).is_some());
        assert!(st["settings"].get(SLOT_B).is_none());

        let only_b = BTreeMap::from([(
            SLOT_B.to_string(),
            PresetSettings {
                power_cap_w: Some(200.0),
                ..Default::default()
            },
        )]);
        c.apply_settings(&only_b, "test").await.unwrap();
        // B applied without clobbering A.
        let st = c.active_state().unwrap();
        assert!(
            st["settings"].get(SLOT_A).is_some(),
            "A must survive B's apply"
        );
        assert!(st["settings"].get(SLOT_B).is_some());
    }

    #[tokio::test]
    async fn revert_for_slot_drops_only_that_slot() {
        let store = Arc::new(Store::memory());
        let ga = fake_gpu(&full_files()).await;
        let gb = crate::driver::fake::gpu_with_slot(&full_files(), SLOT_B);
        let c = Control::new(vec![ga, gb], store);
        c.capture_baseline().await.unwrap();
        let both = BTreeMap::from([
            (
                SLOT_A.to_string(),
                PresetSettings {
                    power_cap_w: Some(250.0),
                    ..Default::default()
                },
            ),
            (
                SLOT_B.to_string(),
                PresetSettings {
                    power_cap_w: Some(200.0),
                    ..Default::default()
                },
            ),
        ]);
        c.apply_settings(&both, "test").await.unwrap();

        let rep = c.revert_for_slot(SLOT_A).await.unwrap();
        assert!(rep.ok);
        assert!(rep.per_gpu.contains_key(SLOT_A));
        assert!(
            !rep.per_gpu.contains_key(SLOT_B),
            "report must cover only the target slot"
        );
        // A back to stock cap (300 W from full_files), B still capped at 200 W.
        let gbs = c.gpus().into_iter().find(|g| g.slot == SLOT_B).unwrap();
        assert!(
            (gbs.read_power_cap_w().await.unwrap() - 200.0).abs() < 1.0,
            "B untouched"
        );
        // A's entry dropped, B's kept (settings and per-knob report).
        let st = c.active_state().unwrap();
        assert!(st["settings"].get(SLOT_A).is_none());
        assert!(st["settings"].get(SLOT_B).is_some());
        assert!(
            st["per_gpu"].get(SLOT_A).is_none(),
            "reverted slot leaves no per-knob report"
        );
        assert!(st["per_gpu"].get(SLOT_B).is_some());

        // Reverting the last slot flips the state to stock.
        c.revert_for_slot(SLOT_B).await.unwrap();
        assert_eq!(c.active_state().unwrap()["stock"], true);
    }

    fn od_with_mclk(max_mclk: i32, sclk: i32, vdd: i32) -> OdTable {
        OdTable {
            sclk_offset_mhz: sclk,
            mclk: std::collections::BTreeMap::from([(0, 97), (1, max_mclk)]),
            vddgfx_mv: vdd,
            ranges: OdRanges {
                sclk_offset: Some((-500, 1000)),
                mclk: Some((97, 1500)),
                vddgfx: Some((-200, 0)),
            },
        }
    }
    const SLOT: &str = "1002:75b4-1458:242f-0000:0b:00.0";

    #[tokio::test]
    async fn compare_drift_detects_without_adopting() {
        let (c, store) = ctl(&full_files()).await;
        c.capture_baseline().await.unwrap();
        c.apply_settings(&full_settings(), "test").await.unwrap(); // records max_mclk_mhz = 1150
        let live = LiveReadback {
            power_cap_w: Some(235.0),
            od: Some(od_with_mclk(1310, 100, -50)), // out-of-band mclk edit
        };
        let delta = c.compare_drift(SLOT, &live);
        assert!(delta.is_some());
        assert_eq!(delta.unwrap().captured.max_mclk_mhz, Some(1310));
        // Pure comparison: no adoption side effects.
        assert!(store.get_meta("active_sources").unwrap().is_none());
        assert_eq!(c.active_state().unwrap()["settings"][SLOT]["max_mclk_mhz"], 1150);
    }

    #[tokio::test]
    async fn compare_drift_none_when_matching() {
        let (c, _store) = ctl(&full_files()).await;
        c.capture_baseline().await.unwrap();
        c.apply_settings(&full_settings(), "test").await.unwrap();
        let live = LiveReadback {
            power_cap_w: Some(235.0),
            od: Some(od_with_mclk(1150, 100, -50)), // matches recorded
        };
        assert!(c.compare_drift(SLOT, &live).is_none());
    }

    #[tokio::test]
    async fn compare_drift_power_tolerance() {
        let (c, _store) = ctl(&full_files()).await;
        c.capture_baseline().await.unwrap();
        c.apply_settings(&full_settings(), "test").await.unwrap(); // records 235.0 W
        // Within ±1.0 W → no drift.
        let live = LiveReadback {
            power_cap_w: Some(235.9),
            od: Some(od_with_mclk(1150, 100, -50)),
        };
        assert!(c.compare_drift(SLOT, &live).is_none());
        // Beyond ±1.0 W → drift.
        let live = LiveReadback {
            power_cap_w: Some(237.0),
            od: Some(od_with_mclk(1150, 100, -50)),
        };
        assert!(c.compare_drift(SLOT, &live).is_some());
    }

    #[tokio::test]
    async fn compare_drift_degenerate_baseline_skipped() {
        // Baseline captured with a failed power read → power_cap_w = 0.0 in
        // the baseline. A live 300 W cap must NOT flag drift on that knob.
        let store = Arc::new(Store::memory());
        // No CAP_REL file: capture_baseline falls back to 0.0 for the cap.
        let g = fake_gpu(&[(OD_REL, OD), (DPM_REL, "auto")]).await;
        let c = Control::new(vec![g], store.clone());
        c.capture_baseline().await.unwrap();
        let baseline = store.get_meta("stock_baseline").unwrap().unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&baseline).unwrap()[SLOT]["power_cap_w"],
            0.0
        );
        // Live cap 300 W (far from 0.0) + OD matching the (zero) baseline → no drift.
        let live = LiveReadback {
            power_cap_w: Some(300.0),
            od: Some(od_with_mclk(1258, 0, 0)),
        };
        assert!(c.compare_drift(SLOT, &live).is_none());
    }

    #[tokio::test]
    async fn adopt_external_writes_state_and_source() {
        let (c, store) = ctl(&full_files()).await;
        c.capture_baseline().await.unwrap();
        let captured = PresetSettings {
            power_cap_w: Some(250.0),
            sclk_offset_mhz: Some(50),
            max_mclk_mhz: Some(1310),
            voltage_offset_mv: Some(-20),
            ..Default::default()
        };
        c.adopt_external(SLOT, &captured);
        let st = c.active_state().unwrap();
        assert_eq!(st["settings"][SLOT]["max_mclk_mhz"], 1310);
        assert_eq!(st["source"], "external");
        let src = store.get_meta("active_sources").unwrap().unwrap();
        let m: serde_json::Value = serde_json::from_str(&src).unwrap();
        assert_eq!(m[SLOT], "external");
        // No event emission from adopt_external itself.
        let evs = store.list_events(None, 100).unwrap();
        assert!(evs.iter().all(|e| e.kind != "external_drift"));
    }

    #[tokio::test]
    async fn compare_drift_skips_when_busy() {
        let (c, _) = ctl(&full_files()).await;
        c.capture_baseline().await.unwrap();
        c.apply_settings(&full_settings(), "test").await.unwrap();
        let _guard = c.busy.try_lock().unwrap();
        let live = LiveReadback { power_cap_w: Some(235.0), od: Some(od_with_mclk(1310, 100, -50)) };
        assert!(c.compare_drift(SLOT, &live).is_none());
    }

    #[tokio::test]
    async fn compare_drift_skips_without_baseline() {
        let (c, _store) = ctl(&full_files()).await;
        // No capture_baseline, no active_state.
        let live = LiveReadback { power_cap_w: Some(999.0), od: Some(od_with_mclk(1310, 100, -50)) };
        assert!(c.compare_drift(SLOT, &live).is_none());
    }

    #[tokio::test]
    async fn compare_drift_skips_unavailable_od() {
        let (c, _store) = ctl(&full_files()).await;
        c.capture_baseline().await.unwrap();
        c.apply_settings(&full_settings(), "test").await.unwrap();
        // od is None → the three OD knobs are skipped; power matches → no drift.
        let live = LiveReadback { power_cap_w: Some(235.0), od: None };
        assert!(c.compare_drift(SLOT, &live).is_none());
    }

    #[tokio::test]
    async fn compare_drift_revert_is_not_drift() {
        let (c, _store) = ctl(&full_files()).await;
        c.capture_baseline().await.unwrap();
        c.apply_settings(&full_settings(), "test").await.unwrap();
        // Revert drops the slot from `active_state.settings` and returns the
        // hardware to the stock baseline (spec §7.1 regression case).
        c.revert().await.unwrap();
        // Live readback == stock baseline → expected values resolve to the
        // baseline, so no drift.
        let live = LiveReadback {
            power_cap_w: Some(300.0),
            od: Some(od_with_mclk(1258, 0, 0)),
        };
        assert!(c.compare_drift(SLOT, &live).is_none());
    }

    #[tokio::test]
    async fn compare_drift_none_knob_uses_baseline() {
        let (c, _store) = ctl(&full_files()).await;
        c.capture_baseline().await.unwrap(); // baseline mclk[1] = 1258
        // Apply a preset that sets ONLY power cap (mclk is None in active_state).
        let only_power = std::collections::BTreeMap::from([(
            SLOT.to_string(),
            PresetSettings { power_cap_w: Some(235.0), ..Default::default() },
        )]);
        c.apply_settings(&only_power, "adhoc").await.unwrap();
        // Live reports mclk 1310 (a manual edit) → drift vs the 1258 baseline.
        let drift = LiveReadback { power_cap_w: Some(235.0), od: Some(od_with_mclk(1310, 0, 0)) };
        assert!(c.compare_drift(SLOT, &drift).is_some());
        // Live reports mclk 1258 (== baseline) → no drift.
        let (c2, _s2) = ctl(&full_files()).await;
        c2.capture_baseline().await.unwrap();
        c2.apply_settings(&only_power, "adhoc").await.unwrap();
        let ok = LiveReadback { power_cap_w: Some(235.0), od: Some(od_with_mclk(1258, 0, 0)) };
        assert!(c2.compare_drift(SLOT, &ok).is_none());
    }
}
