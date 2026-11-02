use std::path::Path;
use std::sync::Mutex;

use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;

use crate::types::{DriftEntry, Event, Preset, PresetSettings};

pub struct Store {
    conn: Mutex<Connection>,
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS presets(
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  name TEXT NOT NULL UNIQUE,
  notes TEXT NOT NULL DEFAULT '',
  settings TEXT NOT NULL,
  is_builtin INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS events(
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  ts INTEGER NOT NULL,
  kind TEXT NOT NULL,
  detail TEXT NOT NULL DEFAULT '{}'
);
CREATE TABLE IF NOT EXISTS meta(
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS drift_ledger(
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  ts INTEGER NOT NULL,
  slot TEXT NOT NULL,
  detected TEXT NOT NULL,
  intended TEXT,
  previous_source TEXT NOT NULL,
  outcome TEXT NOT NULL,
  outcome_detail TEXT
);
"#;

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn migrate(conn: &Connection) -> anyhow::Result<()> {
    // 1. presets.is_builtin column (idempotent).
    let cols: Vec<String> = {
        let mut s = conn.prepare("PRAGMA table_info(presets)")?;
        let v = s.query_map([], |r| r.get::<_, String>(1))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        v
    };
    if !cols.iter().any(|c| c == "is_builtin") {
        conn.execute(
            "ALTER TABLE presets ADD COLUMN is_builtin INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
    }
    // 2. v1 (slot map) -> v2 (single) settings shape, once.
    let done: Option<String> = conn
        .query_row("SELECT value FROM meta WHERE key='presets_shape_v2'", [], |r| r.get(0))
        .optional()?;
    if done.is_none() {
        let rows: Vec<(i64, String)> = {
            let mut s = conn.prepare("SELECT id, settings FROM presets")?;
            let v = s.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            v
        };
        for (id, raw) in rows {
            let v: Value = match serde_json::from_str(&raw) {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!(preset_id = id, "migration: skipping corrupt preset row: {e}");
                    continue;
                }
            };
            let obj = match v.as_object() {
                Some(o) => o,
                None => continue,
            };
            if !obj.keys().any(|k| crate::types::is_pci_slot_id(k)) {
                continue; // already v2 (or pre-slot-map single shape)
            }
            let migrated = if obj.is_empty() {
                Value::Object(Default::default())
            } else {
                let first = obj.keys().min().expect("non-empty object");
                if obj.len() > 1 {
                    tracing::warn!(
                        preset_id = id, slots = obj.len(),
                        "migration: multi-slot preset, keeping first slot {first}"
                    );
                }
                obj.get(first).cloned().unwrap_or(Value::Object(Default::default()))
            };
            conn.execute(
                "UPDATE presets SET settings=? WHERE id=?",
                params![migrated.to_string(), id],
            )?;
        }
        conn.execute(
            "INSERT INTO meta(key, value) VALUES('presets_shape_v2', '1')
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            [],
        )?;
    }
    Ok(())
}

impl Store {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch(SCHEMA)?;
        migrate(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    #[cfg(test)]
    pub fn memory() -> Self {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        migrate(&conn).unwrap();
        Self {
            conn: Mutex::new(conn),
        }
    }

    // ---------- presets ----------

    pub fn create_preset(
        &self,
        name: &str,
        notes: &str,
        settings: &PresetSettings,
    ) -> anyhow::Result<Preset> {
        let now = now_ms();
        let json = serde_json::to_string(settings)?;
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO presets(name, notes, settings, is_builtin, created_at, updated_at)
             VALUES(?1,?2,?3,0,?4,?4)",
            params![name, notes, json, now],
        )
        .map_err(|e| anyhow::anyhow!("insert preset: {e}"))?;
        let id = conn.last_insert_rowid();
        drop(conn);
        self.get_preset(id)
            .and_then(|p| p.ok_or_else(|| anyhow::anyhow!("preset vanished")))
    }

    pub fn get_preset(&self, id: i64) -> anyhow::Result<Option<Preset>> {
        let conn = self.conn.lock().unwrap();
        let row: Option<(i64, String, String, String, i64, i64, i64)> = conn
            .prepare(
                "SELECT id,name,notes,settings,is_builtin,created_at,updated_at FROM presets WHERE id=?",
            )
            .and_then(|mut s| {
                s.query_row(params![id], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, i64>(4)?,
                        r.get::<_, i64>(5)?,
                        r.get::<_, i64>(6)?,
                    ))
                })
            })
            .optional()?;
        drop(conn);
        match row {
            Some((id, name, notes, settings, is_builtin, created_at, updated_at)) => {
                let v: serde_json::Value = serde_json::from_str(&settings)?;
                if !crate::types::is_v2_settings_shape(&v) {
                    return Ok(None);
                }
                Ok(Some(Preset {
                    id: Some(id),
                    name,
                    notes,
                    settings: serde_json::from_value(v)?,
                    is_builtin: is_builtin != 0,
                    created_at: Some(created_at),
                    updated_at: Some(updated_at),
                }))
            }
            None => Ok(None),
        }
    }

    pub fn list_presets(&self) -> anyhow::Result<Vec<Preset>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id,name,notes,settings,is_builtin,created_at,updated_at FROM presets ORDER BY id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, i64>(5)?,
                r.get::<_, i64>(6)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, name, notes, settings, is_builtin, created_at, updated_at) = row?;
            let v: serde_json::Value = match serde_json::from_str(&settings) {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!(preset_id = id, "skipping corrupt preset row: {e}");
                    continue;
                }
            };
            if !crate::types::is_v2_settings_shape(&v) {
                tracing::warn!(preset_id = id, name = %name, "skipping legacy-shape preset row");
                continue;
            }
            let settings: PresetSettings = match serde_json::from_value(v) {
                Ok(s) => s,
                // A v2-shape row whose inner value is broken (e.g. a
                // `power_cap_w` that is not a number) must not brick the
                // whole list — skip it, like the corrupt/legacy rows above.
                Err(e) => {
                    tracing::warn!(preset_id = id, name = %name, "skipping unparseable preset row: {e}");
                    continue;
                }
            };
            out.push(Preset {
                id: Some(id),
                name,
                notes,
                settings,
                is_builtin: is_builtin != 0,
                created_at: Some(created_at),
                updated_at: Some(updated_at),
            });
        }
        Ok(out)
    }

    /// Partial update: `None` fields keep their current value.
    /// Returns `Ok(None)` when the preset id does not exist.
    pub fn update_preset(
        &self,
        id: i64,
        name: Option<&str>,
        notes: Option<&str>,
        settings: Option<&PresetSettings>,
    ) -> anyhow::Result<Option<Preset>> {
        let conn = self.conn.lock().unwrap();
        let current = conn
            .prepare("SELECT name, notes, settings FROM presets WHERE id=?")
            .and_then(|mut s| {
                s.query_row(params![id], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                    ))
                })
            })
            .optional()?;
        let Some((cur_name, cur_notes, cur_settings)) = current else {
            return Ok(None);
        };
        let name = name.unwrap_or(&cur_name).to_string();
        let notes = notes.unwrap_or(&cur_notes).to_string();
        let settings_json = match settings {
            Some(s) => serde_json::to_string(s)?,
            None => cur_settings,
        };
        conn.execute(
            "UPDATE presets SET name=?, notes=?, settings=?, updated_at=? WHERE id=?",
            params![name, notes, settings_json, now_ms(), id],
        )
        .map_err(|e| anyhow::anyhow!("update preset: {e}"))?;
        drop(conn);
        self.get_preset(id)
    }

    pub fn delete_preset(&self, id: i64) -> anyhow::Result<bool> {
        let n = self
            .conn
            .lock()
            .unwrap()
            .execute("DELETE FROM presets WHERE id=?", params![id])
            .map_err(|e| anyhow::anyhow!("delete preset: {e}"))?;
        Ok(n > 0)
    }

    /// Reconcile the shipped built-in presets (spec §5.2): insert missing,
    /// always overwrite existing built-ins, remove built-ins no longer in the
    /// file, never touch a user preset that happens to share a built-in name.
    /// Returns `(inserted, updated, removed)`.
    pub fn upsert_builtins(
        &self,
        builtins: &[crate::builtin::BuiltinPreset],
    ) -> anyhow::Result<(usize, usize, usize)> {
        let conn = self.conn.lock().unwrap();
        let mut inserted = 0usize;
        let mut updated = 0usize;
        for b in builtins {
            let json = serde_json::to_string(&b.settings)?;
            let existing: Option<(i64, i64)> = conn
                .prepare("SELECT id, is_builtin FROM presets WHERE name=?")
                .and_then(|mut s| {
                    s.query_row(params![b.name], |r| Ok((r.get(0)?, r.get(1)?)))
                })
                .optional()?;
            match existing {
                None => {
                    conn.execute(
                        "INSERT INTO presets(name, notes, settings, is_builtin, created_at, updated_at)
                         VALUES(?1,?2,?3,1,?4,?4)",
                        params![b.name, b.notes, json, now_ms()],
                    )
                    .map_err(|e| anyhow::anyhow!("insert built-in preset: {e}"))?;
                    inserted += 1;
                }
                Some((id, 1)) => {
                    conn.execute(
                        "UPDATE presets SET notes=?, settings=?, updated_at=? WHERE id=?",
                        params![b.notes, json, now_ms(), id],
                    )
                    .map_err(|e| anyhow::anyhow!("update built-in preset: {e}"))?;
                    updated += 1;
                }
                Some((_, _)) => {
                    tracing::warn!(name = %b.name, "skipping built-in: user preset with the same name exists");
                }
            }
        }
        let removed = if builtins.is_empty() {
            // Guard: an empty (but parseable) file must never wipe built-ins.
            0
        } else {
            let names: Vec<&str> = builtins.iter().map(|b| b.name.as_str()).collect();
            let ph: Vec<String> = (0..names.len()).map(|i| format!("?{}", i + 1)).collect();
            let sql = format!(
                "DELETE FROM presets WHERE is_builtin=1 AND name NOT IN ({})",
                ph.join(",")
            );
            conn.execute(&sql, rusqlite::params_from_iter(names))
                .map_err(|e| anyhow::anyhow!("remove stale built-in presets: {e}"))?
        };
        Ok((inserted, updated, removed))
    }

    /// Test-only: seed a preset row with raw settings JSON (legacy-shape
    /// rows cannot go through `create_preset`). Returns the new row id.
    #[cfg(test)]
    pub fn insert_raw_preset(
        &self,
        name: &str,
        settings_json: &str,
    ) -> anyhow::Result<i64> {
        let now = now_ms();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO presets(name, notes, settings, created_at, updated_at)
             VALUES(?1, '', ?2, ?3, ?3)",
            params![name, settings_json, now],
        )
        .map_err(|e| anyhow::anyhow!("insert raw preset: {e}"))?;
        Ok(conn.last_insert_rowid())
    }

    /// True if the preset row stores a legacy (v1 slot-map) settings shape.
    pub fn preset_is_legacy(&self, id: i64) -> anyhow::Result<bool> {
        let conn = self.conn.lock().unwrap();
        let raw: Option<String> = conn
            .prepare("SELECT settings FROM presets WHERE id=?")
            .and_then(|mut s| s.query_row(params![id], |r| r.get(0)))
            .optional()?;
        drop(conn);
        match raw {
            Some(s) => {
                let v: serde_json::Value = serde_json::from_str(&s)?;
                Ok(!crate::types::is_v2_settings_shape(&v))
            }
            None => Ok(false),
        }
    }

    // ---------- events ----------

    pub fn append_event(&self, kind: &str, detail: &Value) {
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute(
            "INSERT INTO events(ts, kind, detail) VALUES(?1,?2,?3)",
            params![now_ms(), kind, detail.to_string()],
        );
    }

    pub fn list_events(&self, since: Option<i64>, limit: i64) -> anyhow::Result<Vec<Event>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, ts, kind, detail FROM events
             WHERE (?1 IS NULL OR ts >= ?1) ORDER BY id DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![since, limit], |r| {
            Ok(Event {
                id: r.get(0)?,
                ts: r.get(1)?,
                kind: r.get(2)?,
                detail: serde_json::from_str(r.get::<_, String>(3)?.as_str())
                    .unwrap_or(Value::Object(Default::default())),
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    // ---------- meta ----------

    pub fn get_meta(&self, key: &str) -> anyhow::Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let v = conn
            .prepare("SELECT value FROM meta WHERE key=?")
            .and_then(|mut s| s.query_row(params![key], |r| r.get::<_, String>(0)))
            .optional()?;
        Ok(v)
    }

    pub fn set_meta(&self, key: &str, value: &str) {
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute(
            "INSERT INTO meta(key, value) VALUES(?1,?2)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![key, value],
        );
    }

    pub fn delete_meta(&self, key: &str) {
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute("DELETE FROM meta WHERE key=?", params![key]);
    }

    /// Per-slot source of truth for the UI (spec §6.3): slot ->
    /// "preset:3" | "adhoc" | "stock".
    pub fn set_active_source(&self, slot: &str, source: &str) {
        let mut m: serde_json::Map<String, Value> = self
            .get_meta("active_sources")
            .ok()
            .flatten()
            .and_then(|s| serde_json::from_str(&s).ok())
            .and_then(|v: Value| v.as_object().cloned())
            .unwrap_or_default();
        m.insert(slot.to_string(), Value::String(source.to_string()));
        self.set_meta("active_sources", &Value::Object(m).to_string());
    }

    // ---------- drift ledger ----------

    /// Append a drift episode with `outcome = "pending"` and enforce the
    /// hard cap of 50 rows (newest kept). Returns the new row id.
    pub fn append_drift_entry(
        &self,
        slot: &str,
        detected: &Value,
        intended: &Option<Value>,
        previous_source: &str,
    ) -> anyhow::Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO drift_ledger(ts, slot, detected, intended, previous_source, outcome)
             VALUES(?1,?2,?3,?4,?5,'pending')",
            params![
                now_ms(),
                slot,
                detected.to_string(),
                intended.as_ref().map(Value::to_string),
                previous_source
            ],
        )
        .map_err(|e| anyhow::anyhow!("insert drift entry: {e}"))?;
        let id = conn.last_insert_rowid();
        conn.execute(
            "DELETE FROM drift_ledger WHERE id NOT IN
             (SELECT id FROM drift_ledger ORDER BY id DESC LIMIT 50)",
            [],
        )
        .map_err(|e| anyhow::anyhow!("cap drift ledger: {e}"))?;
        Ok(id)
    }

    /// Delete every drift ledger row. Returns the number of rows removed.
    pub fn clear_drift_ledger(&self) -> anyhow::Result<usize> {
        let conn = self.conn.lock().unwrap();
        let n = conn
            .execute("DELETE FROM drift_ledger", [])
            .map_err(|e| anyhow::anyhow!("clear drift ledger: {e}"))?;
        Ok(n)
    }

    pub fn set_drift_outcome(&self, id: i64, outcome: &str, detail: &Option<Value>) {
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute(
            "UPDATE drift_ledger SET outcome=?, outcome_detail=? WHERE id=?",
            params![outcome, detail.as_ref().map(Value::to_string), id],
        );
    }

    /// Newest first, at most 50 rows (the table is capped at 50).
    pub fn list_drift_entries(&self) -> anyhow::Result<Vec<DriftEntry>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, ts, slot, detected, intended, previous_source, outcome, outcome_detail
             FROM drift_ledger ORDER BY id DESC LIMIT 50",
        )?;
        let rows = stmt.query_map([], |r| {
            let intended: Option<String> = r.get(4)?;
            let detail: Option<String> = r.get(7)?;
            Ok(DriftEntry {
                id: r.get(0)?,
                ts: r.get(1)?,
                slot: r.get(2)?,
                detected: serde_json::from_str(&r.get::<_, String>(3)?)
                    .unwrap_or(Value::Object(Default::default())),
                intended: intended.and_then(|s| serde_json::from_str(&s).ok()),
                previous_source: r.get(5)?,
                outcome: r.get(6)?,
                outcome_detail: detail.and_then(|s| serde_json::from_str(&s).ok()),
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Startup helper (spec §5.8): close every row still in `pending` as
    /// `cleared` — `pending` is never a final state. Returns `(id, slot)` of
    /// the closed rows.
    pub fn close_stale_drift_pending(&self) -> anyhow::Result<Vec<(i64, String)>> {
        let conn = self.conn.lock().unwrap();
        let stale: Vec<(i64, String)> = {
            let mut s = conn.prepare("SELECT id, slot FROM drift_ledger WHERE outcome='pending'")?;
            let v = s.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            v
        };
        if !stale.is_empty() {
            conn.execute("UPDATE drift_ledger SET outcome='cleared' WHERE outcome='pending'", [])?;
        }
        Ok(stale)
    }
}

/// Read the persisted ring window in minutes (default 10, clamped 1–60).
/// Absent or unparseable values (incl. negatives) fall back to the default.
pub fn ring_minutes_from_meta(store: &Store) -> u32 {
    store
        .get_meta("ring_minutes")
        .ok()
        .flatten()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10)
        .clamp(1, 60)
}

/// Read the drift auto-restore flag (default on; `"true"` is on).
pub fn drift_reapply_from_meta(store: &Store) -> bool {
    store
        .get_meta("drift_reapply")
        .ok()
        .flatten()
        .map(|s| s == "true")
        .unwrap_or(true)
}

/// Read the drift re-apply delay in seconds (default 30).
/// Absent or unparseable values fall back to the default. **No clamping
/// here** — the 5–600 range is enforced at the API write path
/// (`POST /api/setup/drift`) and by the guard's delay floor
/// (`DriftGuardConfig::min_delay_s`), so tests can inject sub-5 s delays.
pub fn drift_delay_s_from_meta(store: &Store) -> i64 {
    store
        .get_meta("drift_reapply_delay_s")
        .ok()
        .flatten()
        .and_then(|v| v.parse().ok())
        .unwrap_or(30)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::PresetSettings;

    #[test]
    fn ring_minutes_from_meta_defaults_to_10_when_absent() {
        let s = Store::memory();
        assert_eq!(ring_minutes_from_meta(&s), 10);
    }

    #[test]
    fn ring_minutes_from_meta_falls_back_on_unparseable() {
        let s = Store::memory();
        s.set_meta("ring_minutes", "abc");
        assert_eq!(ring_minutes_from_meta(&s), 10);
        s.set_meta("ring_minutes", "-5");
        assert_eq!(ring_minutes_from_meta(&s), 10);
    }

    #[test]
    fn ring_minutes_from_meta_clamps_to_range() {
        let s = Store::memory();
        s.set_meta("ring_minutes", "0");
        assert_eq!(ring_minutes_from_meta(&s), 1);
        s.set_meta("ring_minutes", "999");
        assert_eq!(ring_minutes_from_meta(&s), 60);
        s.set_meta("ring_minutes", "25");
        assert_eq!(ring_minutes_from_meta(&s), 25);
    }

    #[test]
    fn delete_meta_removes_key_and_is_idempotent() {
        let s = Store::memory();
        s.set_meta("k", "v");
        assert_eq!(s.get_meta("k").unwrap(), Some("v".into()));
        s.delete_meta("k");
        assert_eq!(s.get_meta("k").unwrap(), None);
        s.delete_meta("k"); // deleting a missing key is a no-op
        assert_eq!(s.get_meta("k").unwrap(), None);
    }

    #[test]
    fn preset_crud_roundtrip() {
        let s = Store::memory();
        let settings = PresetSettings {
            power_cap_w: Some(235.0),
            ..Default::default()
        };
        let p = s.create_preset("test", "note", &settings).unwrap();
        assert!(p.id.is_some());
        let got = s.get_preset(p.id.unwrap()).unwrap().unwrap();
        assert_eq!(got.settings, settings);

        let updated = s
            .update_preset(p.id.unwrap(), None, Some("new note"), None)
            .unwrap()
            .unwrap();
        assert_eq!(updated.notes, "new note");
        assert_eq!(updated.settings, settings);

        let all = s.list_presets().unwrap();
        assert_eq!(all.len(), 1);
        assert!(s.delete_preset(p.id.unwrap()).unwrap());
        assert!(s.list_presets().unwrap().is_empty());
        assert!(!s.delete_preset(p.id.unwrap()).unwrap());
    }

    #[test]
    fn preset_name_unique_conflict() {
        let s = Store::memory();
        s.create_preset("dup", "", &PresetSettings::default())
            .unwrap();
        assert!(s.create_preset("dup", "", &PresetSettings::default()).is_err());
    }

    #[test]
    fn update_preset_renames_and_keeps_name_on_none() {
        let s = Store::memory();
        let p = s
            .create_preset("a", "", &PresetSettings { power_cap_w: Some(220.0), ..Default::default() })
            .unwrap();
        let id = p.id.unwrap();

        // Rename keeps notes and settings.
        let up = s.update_preset(id, Some("b"), None, None).unwrap().unwrap();
        assert_eq!(up.name, "b");
        assert_eq!(up.settings.power_cap_w, Some(220.0));

        // name = None keeps the current name.
        let up = s.update_preset(id, None, Some("note"), None).unwrap().unwrap();
        assert_eq!(up.name, "b");
        assert_eq!(up.notes, "note");

        // Rename onto an existing name fails (unique constraint).
        s.create_preset("c", "", &PresetSettings::default()).unwrap();
        assert!(s.update_preset(id, Some("c"), None, None).is_err());
    }

    fn builtin(name: &str, cap: f64) -> crate::builtin::BuiltinPreset {
        crate::builtin::BuiltinPreset {
            name: name.into(),
            notes: "n".into(),
            settings: PresetSettings { power_cap_w: Some(cap), ..Default::default() },
        }
    }

    #[test]
    fn upsert_builtins_inserts_overwrites_skips_user_and_reinserts() {
        let s = Store::memory();
        // 1. Fresh insert.
        let (i, u, r) = s.upsert_builtins(&[builtin("b1", 220.0)]).unwrap();
        assert_eq!((i, u, r), (1, 0, 0));
        let p = s.list_presets().unwrap().into_iter().find(|p| p.name == "b1").unwrap();
        assert!(p.is_builtin);
        assert_eq!(p.settings.power_cap_w, Some(220.0));

        // 2. Built-in overwritten with the newly shipped values (user edits vanish).
        let (i, u, r) = s.upsert_builtins(&[builtin("b1", 230.0)]).unwrap();
        assert_eq!((i, u, r), (0, 1, 0));
        let p = s.list_presets().unwrap().into_iter().find(|p| p.name == "b1").unwrap();
        assert_eq!(p.settings.power_cap_w, Some(230.0));

        // 3. User preset with a built-in name is never touched; the stale
        //    built-in b1 (absent from the file now) is removed.
        s.create_preset("b2", "user", &PresetSettings::default()).unwrap();
        let (i, u, r) = s.upsert_builtins(&[builtin("b2", 100.0)]).unwrap();
        assert_eq!((i, u, r), (0, 0, 1));
        let p = s.list_presets().unwrap().into_iter().find(|p| p.name == "b2").unwrap();
        assert!(!p.is_builtin);
        assert!(s
            .list_presets()
            .unwrap()
            .iter()
            .all(|p| p.name != "b1"));

        // 4. A removed built-in re-inserts on the next start.
        let (i, u, r) = s.upsert_builtins(&[builtin("b1", 220.0)]).unwrap();
        assert_eq!((i, u, r), (1, 0, 0));
    }

    #[test]
    fn upsert_builtins_removes_stale_keeps_users() {
        let s = Store::memory();
        let (i, u, r) = s.upsert_builtins(&[builtin("b1", 220.0), builtin("b2", 210.0)]).unwrap();
        assert_eq!((i, u, r), (2, 0, 0));

        // b1 drops out of the file -> removed; b2 updated.
        let (i, u, r) = s.upsert_builtins(&[builtin("b2", 215.0)]).unwrap();
        assert_eq!((i, u, r), (0, 1, 1));
        let names: Vec<String> = s
            .list_presets()
            .unwrap()
            .iter()
            .map(|p| p.name.clone())
            .collect();
        assert!(names.contains(&"b2".to_string()));
        assert!(!names.contains(&"b1".to_string()));

        // A *user* preset reusing a removed built-in's name survives later upserts.
        s.create_preset("b1", "user", &PresetSettings::default())
            .unwrap();
        let (i, u, r) = s.upsert_builtins(&[builtin("b2", 215.0)]).unwrap();
        assert_eq!((i, u, r), (0, 1, 0));
        let p = s
            .list_presets()
            .unwrap()
            .into_iter()
            .find(|p| p.name == "b1")
            .unwrap();
        assert!(!p.is_builtin);
    }

    #[test]
    fn drift_ledger_append_and_list_newest_first() {
        let s = Store::memory();
        let id1 = s.append_drift_entry("slot-a", &serde_json::json!({"power_cap_w": 250.0}),
            &Some(serde_json::json!({"power_cap_w": 235.0})), "adhoc").unwrap();
        let id2 = s.append_drift_entry("slot-a", &serde_json::json!({"power_cap_w": 300.0}),
            &None, "stock").unwrap();
        assert_eq!(id2, id1 + 1);
        let rows = s.list_drift_entries().unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].id, id2); // newest first
        assert_eq!(rows[0].slot, "slot-a");
        assert_eq!(rows[0].previous_source, "stock");
        assert_eq!(rows[0].outcome, "pending");
        assert_eq!(rows[0].intended, None);
        assert_eq!(rows[0].outcome_detail, None);
        assert_eq!(rows[1].intended, Some(serde_json::json!({"power_cap_w": 235.0})));
    }

    #[test]
    fn drift_ledger_caps_at_50_newest() {
        let s = Store::memory();
        for i in 0..60 {
            s.append_drift_entry("slot-a", &serde_json::json!({ "i": i }), &None, "stock")
                .unwrap();
        }
        let rows = s.list_drift_entries().unwrap();
        assert_eq!(rows.len(), 50);
        // The 10 oldest (i = 0..10) are gone; newest (i = 59) is first.
        assert_eq!(rows[0].detected["i"], 59);
        assert_eq!(rows[49].detected["i"], 10);
    }

    #[test]
    fn drift_ledger_clear() {
        let s = Store::memory();
        for _ in 0..3 {
            s.append_drift_entry("slot-a", &serde_json::json!({ "power_cap_w": 250.0 }), &None, "stock")
                .unwrap();
        }
        assert_eq!(s.clear_drift_ledger().unwrap(), 3);
        assert!(s.list_drift_entries().unwrap().is_empty());
        // A further append works and is listed (ids continue).
        let id = s
            .append_drift_entry("slot-a", &serde_json::json!({ "power_cap_w": 300.0 }), &None, "stock")
            .unwrap();
        let rows = s.list_drift_entries().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, id);
    }

    #[test]
    fn drift_set_outcome_updates_row() {
        let s = Store::memory();
        let id = s.append_drift_entry("slot-a", &serde_json::json!({}), &None, "adhoc").unwrap();
        s.set_drift_outcome(id, "reapplied",
            &Some(serde_json::json!([{"field": "power_cap_w", "status": "verified", "detail": "x"}])));
        let rows = s.list_drift_entries().unwrap();
        assert_eq!(rows[0].outcome, "reapplied");
        assert_eq!(rows[0].outcome_detail,
            Some(serde_json::json!([{"field": "power_cap_w", "status": "verified", "detail": "x"}])));
    }

    #[test]
    fn drift_close_stale_pending_closes_only_pending_rows() {
        let s = Store::memory();
        let a = s.append_drift_entry("slot-a", &serde_json::json!({}), &None, "stock").unwrap();
        let b = s.append_drift_entry("slot-b", &serde_json::json!({}), &None, "stock").unwrap();
        s.set_drift_outcome(a, "reapplied", &None);
        let closed = s.close_stale_drift_pending().unwrap();
        assert_eq!(closed, vec![(b, "slot-b".to_string())]);
        let rows = s.list_drift_entries().unwrap();
        assert_eq!(rows[0].outcome, "cleared");
        assert_eq!(rows[1].outcome, "reapplied");
        // Idempotent: nothing pending left.
        assert!(s.close_stale_drift_pending().unwrap().is_empty());
    }

    #[test]
    fn drift_meta_defaults() {
        let s = Store::memory();
        assert!(drift_reapply_from_meta(&s)); // absent → on
        assert_eq!(drift_delay_s_from_meta(&s), 30); // absent → 30
        s.set_meta("drift_reapply", "false");
        assert!(!drift_reapply_from_meta(&s));
        s.set_meta("drift_reapply", "true");
        assert!(drift_reapply_from_meta(&s));
        s.set_meta("drift_reapply_delay_s", "abc");
        assert_eq!(drift_delay_s_from_meta(&s), 30); // unparseable → 30
        s.set_meta("drift_reapply_delay_s", "45");
        assert_eq!(drift_delay_s_from_meta(&s), 45);
        // The 5–600 range is NOT enforced here (API write path + guard
        // floor own it) — the reader passes stored values through.
        s.set_meta("drift_reapply_delay_s", "601");
        assert_eq!(drift_delay_s_from_meta(&s), 601);
    }

    #[test]
    fn events_and_meta() {
        let s = Store::memory();
        s.append_event("apply", &serde_json::json!({"ok": true}));
        s.append_event("revert", &serde_json::json!({}));
        let evs = s.list_events(None, 10).unwrap();
        assert_eq!(evs.len(), 2);
        assert_eq!(evs[0].kind, "revert"); // newest first

        s.set_meta("stock_baseline_ts", "123");
        assert_eq!(s.get_meta("stock_baseline_ts").unwrap(), Some("123".into()));
        s.set_meta("stock_baseline_ts", "456");
        assert_eq!(s.get_meta("stock_baseline_ts").unwrap(), Some("456".into()));
    }

    const OLD_SCHEMA: &str = r#"
    CREATE TABLE presets(
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      name TEXT NOT NULL UNIQUE,
      notes TEXT NOT NULL DEFAULT '',
      settings TEXT NOT NULL,
      created_at INTEGER NOT NULL,
      updated_at INTEGER NOT NULL
    );
    CREATE TABLE trials(
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      preset_id INTEGER NOT NULL REFERENCES presets(id),
      profile_id INTEGER NOT NULL REFERENCES benchmark_profiles(id),
      notes TEXT NOT NULL DEFAULT '',
      status TEXT NOT NULL,
      started_at INTEGER NOT NULL,
      ended_at INTEGER,
      benchmark TEXT,
      thermal TEXT,
      error TEXT
    );
    CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);
    "#;

    fn old_db(path: &std::path::Path, rows: &[(&str, &str)]) -> i64 {
        let conn = rusqlite::Connection::open(path).unwrap();
        conn.execute_batch(OLD_SCHEMA).unwrap();
        let mut id = 0;
        for (name, settings) in rows {
            conn.execute(
                "INSERT INTO presets(name, notes, settings, created_at, updated_at)
                 VALUES(?1,'',?2,1,1)",
                params![name, settings],
            )
            .unwrap();
            id += 1;
        }
        id
    }

    #[test]
    fn migration_converts_v1_rows_and_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.db");
        old_db(
            &path,
            &[
                ("one", r#"{"1002:75b4-1458:242f-0000:0b:00.0":{"power_cap_w":250.0}}"#),
                ("two", r#"{"1002:75b4-1458:242f-0000:0a:00.0":{"power_cap_w":200.0},"1002:75b4-1458:242f-0000:0b:00.0":{"sclk_offset_mhz":10}}"#),
                ("empty", r#"{}"#),
                ("v2", r#"{"voltage_offset_mv":-80}"#),
            ],
        );
        let s = Store::open(&path).unwrap();
        let one = s.list_presets().unwrap().into_iter().find(|p| p.name == "one").unwrap();
        assert_eq!(one.settings.power_cap_w, Some(250.0));
        assert!(!one.is_builtin);
        // two-slot row keeps the lexicographically first slot's value.
        let two = s.list_presets().unwrap().into_iter().find(|p| p.name == "two").unwrap();
        assert_eq!(two.settings.power_cap_w, Some(200.0));
        // already-v2 row untouched.
        let v2 = s.list_presets().unwrap().into_iter().find(|p| p.name == "v2").unwrap();
        assert_eq!(v2.settings.voltage_offset_mv, Some(-80));
        // Second open: no double migration, data stable.
        drop(s);
        let s2 = Store::open(&path).unwrap();
        let one2 = s2.list_presets().unwrap().into_iter().find(|p| p.name == "one").unwrap();
        assert_eq!(one2.settings.power_cap_w, Some(250.0));
    }
}
