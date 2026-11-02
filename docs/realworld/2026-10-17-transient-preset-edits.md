# Real-World Check: GpuTile transient preset edits — modified, Save, switch guard, leave guard, panel delete guard

Date: 2026-10-17
Branch/commits: `feat/chart-legend-layout` at `39084d2` (includes
`1a4252a` transient edits + Save + switch guard + tile-delete removal,
`1c7ce24` pending-store registration (leave-dashboard guard),
`08db501` Presets-panel in-use delete guard,
`39084d2` keep transient edits visible after per-slot activeSettings catches up)
Hardware: 4× real GPUs enumerated; this check uses two:
  - card A = `1002:7551-1458:242f-0000:0b:00.0`
  - card B = `1002:7551-1458:242f-0000:0c:00.0`

This entry is written as a **replayable** procedure: each check lists imperative
steps, the expected outcome, and exactly how to verify it (selector / API call /
value). Re-run it after any change to `ui/src/lib/components/GpuTile.svelte`,
`ui/src/lib/store/pending.svelte`, `ui/src/routes/Presets.svelte`, or
`ui/src/App.svelte`.

It supersedes the auto-persist / tile-trash expectations in
`2026-10-12-gpu-tile-preset-clone-edit-delete.md` (Checks 1, 2, 3, 5 there are
marked superseded; Check 4 — API delete heal — is backend and unchanged).

## What is covered

Five behaviors of the transient-edit model:

1. **Transient edit** → committing a knob applies it to the GPU live but does
   **not** persist to the preset; the field keeps the committed value across
   real telemetry ticks (no jump-back to the saved value), and the Save button
   becomes enabled. This includes the regression fixed in `39084d2`: before it,
   the field snapped back to the saved preset value as soon as the per-slot
   `active_state` caught up.
2. **Save (merge)** → the Save button persists the four tile knobs into the
   preset **without wiping** the preset's fan / thermal-target / power-profile
   settings; afterwards the tile is no longer `modified`.
3. **Preset-switch guard** → picking a different preset (or stock) while
   `modified` opens an "Unsaved changes" dialog with **Save / Stay / Dismiss**
   instead of switching immediately.
4. **Leave-Dashboard guard** → leaving the Dashboard via the sidebar while a
   tile is `modified` opens the "Unsaved preset changes" dialog
   (Stay / Discard / Save) over all modified tiles.
5. **Presets-panel delete guard** → deleting a preset any GPU is on is blocked
   by a "Can't delete preset" modal; unused presets delete as before. The tile's
   trash button no longer exists.

## Setup

The served UI is the `rust-embed` compile-time bundle, so rebuild both artifacts
before starting, then start the daemon (passwordless sudo — see `README.md`):

```bash
cd ui && npm run build        # -> dist/assets/index-*.js
cargo build --release         # re-embeds ui/dist
sh scripts/restart-amd-gpu-tuned.sh   # from repo root (rebuild + restart)
```

Confirm `curl -s localhost:8787/api/status` returns `ok:true` and entries in
`active_sources`. Drive the UI with the Playwright MCP browser tools against
`http://127.0.0.1:8787/`.

### Helpers used throughout

- `A` = `1002:7551-1458:242f-0000:0b:00.0`, `B` = `1002:7551-1458:242f-0000:0c:00.0`.
- `sources()` = `curl -s localhost:8787/api/status | jq -c .active_sources`.
- `applied <slot>` = `curl -s localhost:8787/api/status | jq -c .active_state.settings.<slot>`.
- `mk <name>` = `curl -s -X POST localhost:8787/api/presets -H 'content-type: application/json' -d '{"name":"<name>","settings":{"power_cap_w":210.0}}' | jq .id`.
- `mkfan <name>` = same as `mk` but with a static fan, so Save-merge has a
  non-null fan to preserve:
  `curl -s -X POST localhost:8787/api/presets -H 'content-type: application/json' -d '{"name":"<name>","settings":{"power_cap_w":210.0,"max_mclk_mhz":1350.0,"fan":{"enabled":true,"mode":"static","static_speed":0.5,"curve":null}}}' | jq .id`.
- `apply <id> <slot>` = `curl -s -X POST localhost:8787/api/apply -H 'content-type: application/json' -d '{"preset_id":<id>,"gpu":"<slot>"}'`.
- `del <id>` = `curl -s -o /dev/null -w '%{http_code}' -X DELETE localhost:8787/api/presets/<id>`.
- UI selectors: `tile-preset` (combo), `tile-preset-add` (clone),
  `tile-preset-save` (Save — **replaces the removed trash button**),
  `tile-power-cap`, `tile-sclk`, `tile-mclk`, `tile-preset-opt-<id>`
  (dropdown option), `tile-switch-stay` / `tile-switch-dismiss` /
  `tile-switch-save` (switch dialog), `unsaved-stay` / `unsaved-discard` /
  `unsaved-save` (leave-dashboard dialog), `nav-presets` / `nav-dashboard`
  (sidebar), `preset-row-<id>` (Presets panel row), `[role="dialog"]` (modal).

Start each check from a known state: revert both cards to stock and delete any
leftover `rw-*` presets from a prior run.

---

## Check 1: Transient edit — live apply, no persist, no jump-back

**Goal:** committing a knob on a user preset applies it to the GPU immediately,
does **not** write the preset, and the field keeps the new value across real
telemetry ticks (the `39084d2` regression: the field used to snap back to the
saved value once the per-slot `active_state` caught up).

**Steps:**
1. `ID=$(mkfan rw-transient)` ; `apply $ID $A`.
2. Navigate to the dashboard; wait ~2 s for the preset list to load.
3. Confirm card A's `tile-preset` reads `rw-transient` and `tile-mclk` reads
   `1350`; `tile-preset-save` is **disabled** (nothing modified yet).
4. In card A's `tile-mclk`, type `1320`, then press **Tab** (blur-commit — do
   NOT press Enter).
5. Wait ~6 s (≥2 real telemetry ticks + one 5 s status poll).
6. Read `tile-mclk` again.

**Expected outcome:**
- The GPU is running 1320 MHz **live**.
- The preset's stored `max_mclk_mhz` is **still 1350** — nothing was persisted.
- The field reads `1320` (NOT reverted to `1350`).
- `tile-preset-save` is now **enabled** (`modified` is true).
- Card A's `active_source` is still `preset:$ID` (not `adhoc`).

**How to verify:**
- `applied A` → `max_mclk_mhz` == `1320` (per-slot live state).
- `curl -s localhost:8787/api/presets/$ID | jq .settings.max_mclk_mhz` → `1350`.
- UI: `tile-mclk` value == `1320` after the 6 s wait;
  `tile-preset-save` not disabled.
- `sources()` → `"$A": "preset:$ID"`.

**Cleanup:** `apply $ID $A` (re-apply saved values — the live 1320 is discarded);
then continue to Check 2 on the same preset.

---

## Check 2: Save — merges the four knobs, preserves fan

**Goal:** the Save button writes the four tile knobs into the preset **on top
of** its existing settings — the preset's fan (and thermal / power-profile)
must survive.

**Steps:**
1. Continue from Check 1 (card A on `rw-transient`, fan static 0.5, mclk saved
   1350). Re-commit the transient edit: type `1320` into `tile-mclk`, Tab.
2. Click card A's Save button (`tile-preset-save`).
3. Wait ~2 s for the preset-list refresh.

**Expected outcome:**
- The preset's `max_mclk_mhz` is now `1320`.
- The preset's `fan` is **unchanged** (`{"enabled":true,"mode":"static",
  "static_speed":0.5,"curve":null}`) — not wiped to `null`.
- The tile is no longer `modified`: `tile-preset-save` is **disabled** again.
- `tile-mclk` still reads `1320`.

**How to verify:**
- `curl -s localhost:8787/api/presets/$ID | jq .settings` → `max_mclk_mhz`
  `1320`, `fan` exactly as created, `power_cap_w` `210.0`.
- UI: `tile-preset-save` disabled; `tile-mclk` == `1320`.

**Cleanup:** `del $ID` (card A reverts to stock via the heal).

---

## Check 3: Preset-switch guard — Save / Stay / Dismiss

**Goal:** picking a different preset while `modified` opens an "Unsaved
changes" dialog naming the current preset, and each of the three buttons
behaves as specified.

**Setup for each sub-check:** `P1=$(mkfan rw-switch-a)` ; `P2=$(mk rw-switch-b)`
; `apply $P1 $A` ; dashboard ; wait ~2 s. Then make card A `modified`: type
`218` into `tile-power-cap`, Tab. (Saved value is 210.)

**3a — Stay cancels the switch:**
1. Click `tile-preset`, click option `tile-preset-opt-$P2`.
2. Observe the dialog; click **Stay** (`tile-switch-stay`).

- Expected: dialog closes; card A is **still on** `rw-switch-a`; `tile-power-cap`
  still reads `218`; `tile-preset-save` still enabled; `P1` **not** updated.
- Verify: `sources()` → `"$A": "preset:$P1"`;
  `curl -s localhost:8787/api/presets/$P1 | jq .settings.power_cap_w` → `210.0`.

**3b — Dismiss discards, then switches:**
1. (Card A still modified at 218.) Click `tile-preset`, click
   `tile-preset-opt-$P2`.
2. Click **Dismiss** (`tile-switch-dismiss`).

- Expected: dialog closes; card A is now on `rw-switch-b`; the transient 218 is
  discarded (card A's power cap re-applied at `P1`'s saved 210 before the
  switch); `P1` **not** updated.
- Verify: `sources()` → `"$A": "preset:$P2"`;
  `curl -s localhost:8787/api/presets/$P1 | jq .settings.power_cap_w` → `210.0`;
  `applied A` → `power_cap_w` == `210.0` (then the P2 apply settles it to P2's
  210 — same value here; the assertion is that `P1` was never written).

**3c — Save persists, then switches:**
1. `apply $P1 $A` ; wait ~1 s ; re-modify: type `218` into `tile-power-cap`,
   Tab.
2. Click `tile-preset`, click `tile-preset-opt-$P2`.
3. Click **Save** (`tile-switch-save`).

- Expected: dialog closes; `P1`'s stored `power_cap_w` is now `218`; card A is
  on `rw-switch-b`.
- Verify: `curl -s localhost:8787/api/presets/$P1 | jq .settings.power_cap_w`
  → `218.0`; `sources()` → `"$A": "preset:$P2"`.

**Cleanup:** `del $P1` ; `del $P2` (card A reverts to stock via the heal).

---

## Check 4: Leave-Dashboard guard — Stay / Discard / Save

**Goal:** leaving the Dashboard **via the sidebar** while a tile is `modified`
opens the "Unsaved preset changes" dialog. Note: the guard intercepts sidebar
link clicks — navigating by typing `#/presets` into the URL bar does **not**
trigger it (by design; only in-app navigation is guarded).

**Setup for each sub-check:** `ID=$(mkfan rw-leave)` ; `apply $ID $A` ;
dashboard ; wait ~2 s ; type `218` into card A's `tile-power-cap`, Tab
(`modified` — `tile-preset-save` enabled).

**4a — Stay:**
1. Click the sidebar **Presets** link (`nav-presets`).
2. Observe the dialog; click **Stay** (`unsaved-stay`).

- Expected: dialog titled `Unsaved preset changes` names `rw-leave`; after
  Stay, the dialog closes and the route is **still the dashboard**;
  `tile-power-cap` still `218`.
- Verify: `location.hash` == `#/` (or empty); `tile-power-cap` == `218`.

**4b — Discard:**
1. (Still modified.) Click `nav-presets` again; in the dialog click
   **Discard** (`unsaved-discard`).

- Expected: navigation proceeds to the Presets page; card A's transient 218 is
  discarded (power cap re-applied at the saved 210); `ID` **not** updated.
- Verify: `location.hash` == `#/presets`;
  `curl -s localhost:8787/api/presets/$ID | jq .settings.power_cap_w` → `210.0`.

**4c — Save:**
1. Back to the dashboard (`nav-dashboard`); re-modify: type `218` into
   `tile-power-cap`, Tab.
2. Click `nav-presets`; in the dialog click **Save** (`unsaved-save`).

- Expected: navigation proceeds; `ID`'s stored `power_cap_w` is now `218`.
- Verify: `location.hash` == `#/presets`;
  `curl -s localhost:8787/api/presets/$ID | jq .settings.power_cap_w` → `218.0`.

**Cleanup:** `del $ID` (card A reverts to stock via the heal).

---

## Check 5: Presets-panel delete guard + tile trash removed

**Goal:** the tile no longer has a trash button, and the Presets panel blocks
deleting a preset any GPU is on ("switch them off it first") while unused
presets still delete through the confirm dialog.

**Steps:**
1. `P1=$(mkfan rw-panel-guard)` ; `apply $P1 $A` ; `apply $P1 $B`.
2. Dashboard: verify the tile has **no** trash button and **has** a Save
   button.
3. Go to the Presets page (sidebar). Find row `preset-row-$P1`, hover its last
   cell (row actions are hidden until hover), click the **delete** icon.
4. Observe the modal; click **Dismiss**.
5. Revert both cards: `curl -s -X POST localhost:8787/api/revert -H
   'content-type: application/json' -d '{"gpu":"'$A'"}'` and same for `$B`
   (or click `tile-preset-opt-default` on both tiles). Wait ~1 s.
6. On the Presets page, click the row's **delete** icon again; in the confirm
   dialog click **delete**.

**Expected outcome:**
- Step 2: `[data-testid="tile-preset-delete"]` does not exist;
  `[data-testid="tile-preset-save"]` does.
- Step 4: a `role="dialog"` titled `Can't delete preset` with body
  `'rw-panel-guard' is applied to 2 GPUs. Switch them off it first.` and a
  **Dismiss** button; the preset is **not** deleted.
- Step 6: the normal `Delete preset` confirm dialog appears (NOT the blocked
  modal) and deleting succeeds.

**How to verify:**
- Step 4: `[role="dialog"]` present; text contains `Can't delete` and
  `rw-panel-guard`; `curl -s -o /dev/null -w '%{http_code}'
  localhost:8787/api/presets/$P1` → `200` (still exists);
  `sources()` → both cards still `preset:$P1`.
- Step 6: `del` via UI → `curl -s -o /dev/null -w '%{http_code}'
  localhost:8787/api/presets/$P1` → `404`; `sources()` → both cards `stock`.

**Cleanup:** none (preset deleted; both cards at stock).

---

## GPU state impact / restore

Every check above changes GPU state (apply/revert) and creates `rw-*` test
presets. After the full run, restore a clean state:

```bash
# delete any leftover test presets (each reverts its GPU to stock via the heal)
for id in $(curl -s localhost:8787/api/presets | jq '.[] | select(.name|startswith("rw-")) | .id'); do
  curl -s -o /dev/null -X DELETE localhost:8787/api/presets/$id
done
# belt-and-braces: revert the two cards this check used
curl -s -X POST localhost:8787/api/revert -H 'content-type: application/json' -d '{"gpu":"1002:7551-1458:242f-0000:0b:00.0"}'
curl -s -X POST localhost:8787/api/revert -H 'content-type: application/json' -d '{"gpu":"1002:7551-1458:242f-0000:0c:00.0"}'
```

Confirm `sources()` shows both cards at `stock` and no `rw-*` presets remain.
Leave the user's own presets untouched.

## Verdict template

- [ ] Check 1 (transient edit: live apply, no persist, no jump-back, Save enables): PASS / FAIL
- [ ] Check 2 (Save merges 4 knobs, preserves fan, clears modified): PASS / FAIL
- [ ] Check 3a (switch dialog: Stay cancels): PASS / FAIL
- [ ] Check 3b (switch dialog: Dismiss discards + switches, no persist): PASS / FAIL
- [ ] Check 3c (switch dialog: Save persists + switches): PASS / FAIL
- [ ] Check 4a (leave guard: Stay stays on dashboard): PASS / FAIL
- [ ] Check 4b (leave guard: Discard navigates, no persist): PASS / FAIL
- [ ] Check 4c (leave guard: Save navigates, persists): PASS / FAIL
- [ ] Check 5 (tile trash gone; panel blocks in-use delete; unused deletes): PASS / FAIL
- [ ] Restore: both cards at stock, no `rw-*` presets: PASS / FAIL
