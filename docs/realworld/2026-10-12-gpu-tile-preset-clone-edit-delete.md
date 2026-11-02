# Real-World Check: GpuTile preset clone / edit / delete (heal, blur-commit, label, multi-card guard)

Date: 2026-10-12
Branch/commits: `main` at `c0340c7` (includes
`bc51891` heal dangling source + commit-on-blur + apply-through-preset_id,
`561b2fc` block tile delete when other GPUs use the preset,
`c0340c7` blocked-delete shown in a prominent red dismissible modal)
Hardware: 2× real GPUs enumerated
  - card A = `1002:7551-1458:242f-0000:0b:00.0`
  - card B = `1002:7551-1458:242f-0000:45:00.0`

This entry is written as a **replayable** procedure: each check lists imperative
steps, the expected outcome, and exactly how to verify it (selector / API call /
value). Re-run it after any change to `ui/src/lib/components/GpuTile.svelte`,
`ui/src/routes/Dashboard.svelte`, `src/api/presets.rs`, or `src/persist.rs`.

> **Superseded in part (2026-10-17).** The transient-edit model
> (`2026-10-17-transient-preset-edits.md`) changed what this entry's Checks 1,
> 2, 3 and 5 assume: knob edits are now **transient** (applied live, persisted
> only on an explicit **Save** — there is no auto-persist), and the **tile
> trash button is gone** (delete lives in the Presets panel, with an in-use
> guard). Those checks are marked inline below and no longer pass as written;
> the new entry's Checks 1–5 cover the current behavior. Check 4 (API delete
> heal) is backend and still valid.

## What is covered

Six behaviors fixed in this session, all in the preset clone/edit/delete flow:

1. **Clone + edit via blur** → the typed value is committed (not just Enter), it
   persists to the clone, and the tile's label stays on the clone preset
   (`active_source` stays `preset:<id>`, not `adhoc`).
2. **Multi-card delete guard** → deleting a preset that another GPU still uses
   is blocked by a prominent, red, must-be-dismissed centered modal (not a
   corner toast).
3. **Solo delete** → when only this GPU uses the preset, the normal confirm
   dialog deletes it and reverts the card to stock.
4. **Heal on delete** → deleting a preset a GPU is on (via the API / Presets
   page) reverts that GPU to stock instead of leaving a dangling `preset:<id>`.
5. **Shared-preset propagation** → with two GPUs on the same preset, editing a
   knob through one GPU updates the *shared preset*, and the other GPU's tile
   reflects the new value immediately (its display derives from the live preset,
   not its own stale last-applied per-slot value).
6. **Selection sync (unit-tested, noted here)** → a tile that mounts before the
   preset list loads adopts the active preset once the list arrives (no stuck
   "custom"). This is a load race, so it is covered by the GpuTile unit test
   `adopts the active preset once the list loads after mount` rather than a
   deterministic UI replay.

## Setup

The served UI is the `rust-embed` compile-time bundle, so rebuild both artifacts
before starting, then start the daemon (passwordless sudo — see `README.md`):

```bash
cd ui && npm run build        # -> dist/assets/index-*.js
cargo build --release         # re-embeds ui/dist
sh scripts/restart-amd-gpu-tuned.sh   # from repo root (rebuild + restart)
```

Confirm `curl -s localhost:8787/api/status` returns `ok:true` and two entries in
`active_sources`. Drive the UI with the Playwright MCP browser tools against
`http://127.0.0.1:8787/`.

### Helpers used throughout

- `A` = `1002:7551-1458:242f-0000:0b:00.0`, `B` = `1002:7551-1458:242f-0000:45:00.0`.
- `sources()` = `curl -s localhost:8787/api/status | jq -c .active_sources`.
- `mk <name>` = `curl -s -X POST localhost:8787/api/presets -H 'content-type: application/json' -d '{"name":"<name>","settings":{"power_cap_w":210.0}}' | jq .id`.
- `apply <id> <slot>` = `curl -s -X POST localhost:8787/api/apply -H 'content-type: application/json' -d '{"preset_id":<id>,"gpu":"<slot>"}'`.
- `del <id>` = `curl -s -o /dev/null -w '%{http_code}' -X DELETE localhost:8787/api/presets/<id>`.
- UI selectors: `tile-preset` (combo), `tile-preset-add` (clone / "New preset"),
  `tile-power-cap`, `tile-preset-opt-<id>` (dropdown option),
  `[role="dialog"]` (modal), Dismiss / Delete buttons.
  (The `tile-preset-delete` trash selector no longer exists — replaced by
  `tile-preset-save` on 2026-10-17.)

Start each check from a known state: revert both cards to stock and delete any
leftover `rw-*` presets from a prior run.

---

## Check 1: Clone → edit via blur → value persists, label stays on the clone

> **Superseded (2026-10-17):** the blur-commit still happens, but the value is
> now **transient** — it is applied live and does **not** persist to the clone
> until the Save button (`tile-preset-save`) is clicked. The "clone's stored
> `power_cap_w` is 218" expectation no longer holds without a Save. See
> `2026-10-17-transient-preset-edits.md` Checks 1–2.

**Goal:** cloning a preset, then editing a field and *blurring* (not pressing
Enter), commits the value to the clone and keeps the tile on the clone preset.

**Steps:**
1. `ID=$(mk rw-clone-test)` ; `apply $ID $A`.
2. Navigate to the dashboard; wait ~2 s for the preset list to load.
3. On card A, confirm the combo (`tile-preset`) reads `rw-clone-test`.
4. Click card A's clone button (`tile-preset-add`).
5. In card A's `tile-power-cap`, type `218`, then press **Tab** (blur — do NOT
   press Enter).
6. Wait ~2 s for the apply + persist to settle.

**Expected outcome:**
- A clone named `rw-clone-test (1)` now exists; card A is on it.
- Card A's power cap reads `218` (not reverted to `210`).
- The clone's stored `power_cap_w` is `218`.
- Card A's `active_source` is **still** `preset:<clone>` — NOT `adhoc`.

**How to verify:**
- `sources()` → `"$A": "preset:<clone>"` where `<clone>` is the new id (find it
  as the newest id in `curl -s localhost:8787/api/presets | jq '.[].id'`).
- `curl -s localhost:8787/api/presets/<clone> | jq .settings.power_cap_w` → `218`.
- UI: `tile-power-cap` value == `218`; `tile-preset` value == `rw-clone-test (1)`.

**Cleanup:** `del <clone>` (also deletes the original if you created it fresh);
revert card A to stock.

---

## Check 2: Multi-card delete is blocked by a prominent red modal

> **Superseded (2026-10-17):** the tile trash button was removed; the in-use
> delete guard now lives in the **Presets panel** (`preset-delete-blocked`
> modal, "Switch them off it first"). See
> `2026-10-17-transient-preset-edits.md` Check 5.

**Goal:** with two GPUs on the same preset, the tile trash button refuses to
delete and shows a centered, red, must-be-dismissed modal (no corner toast).

**Steps:**
1. `ID=$(mk rw-shared-test)` ; `apply $ID $A` ; `apply $ID $B`.
2. Navigate to the dashboard; wait ~2 s.
3. Confirm both cards' `tile-preset` read `rw-shared-test`.
4. Click card A's trash button (`tile-preset-delete`).
5. Observe the modal, then dismiss it (click **Dismiss**, or the `✕`, press
   **Escape**, or click the backdrop).

**Expected outcome:**
- A centered `role="dialog"` appears over a dimmed backdrop.
- Title: `Can't delete preset`.
- Body (in **red**): `'rw-shared-test' is also applied to 1 other GPU. Switch
  them off it first.`
- A **Dismiss** button; the preset is **not** deleted; both cards are
  unaffected.
- Dismiss closes the modal.

**How to verify:**
- `document.querySelector('[role="dialog"]')` is present after the click.
- `[role="dialog"] p` text == `'rw-shared-test' is also applied to 1 other GPU.
  Switch them off it first.` and `getComputedStyle(p).color` is the danger red
  (`rgb(255, 103, 100)`, class `text-danger`).
- `del $ID` is NOT called by the UI: `curl -s -o /dev/null -w '%{http_code}'
  localhost:8787/api/presets/$ID` → `200` (still exists).
- `sources()` → both `"$A"` and `"$B"` still `preset:$ID`.
- After Dismiss: `document.querySelector('[role="dialog"]')` is `null`.
- No error toast: the corner toast stack is empty.

**Cleanup:** `del $ID` (library delete reverts both cards to stock via the heal).

---

## Check 3: Solo delete works (confirm dialog → delete → card reverts to stock)

> **Superseded (2026-10-17):** deletion is now only reachable from the
> **Presets panel** (the tile trash button is gone). An unused preset still
> deletes through the confirm dialog; an in-use one is blocked. See
> `2026-10-17-transient-preset-edits.md` Check 5.

**Goal:** when only this GPU uses the preset, the trash button opens the normal
confirm dialog and deleting reverts the card to stock.

**Steps:**
1. `ID=$(mk rw-solo-test)` ; `apply $ID $A` (card B stays on stock).
2. Navigate to the dashboard; wait ~2 s.
3. Click card A's trash button (`tile-preset-delete`).
4. In the confirm dialog, click **Delete**.

**Expected outcome:**
- A `role="dialog"` titled `Delete preset` appears with a **Delete** button
  (this is the confirm dialog, NOT the "Can't delete" blocked modal).
- After confirming: the preset is deleted and card A is reverted to stock.

**How to verify:**
- Before confirming: `[role="dialog"]` heading == `Delete preset`; a button
  named `Delete` exists.
- After confirming: `curl -s -o /dev/null -w '%{http_code}'
  localhost:8787/api/presets/$ID` → `404`.
- `sources()` → `"$A": "stock"`.
- UI: card A's `tile-preset` value == `stock`.

**Cleanup:** none (preset already deleted; card A at stock).

---

## Check 4: Deleting a preset a GPU is on (API) heals the dangling state

**Goal:** a library/API delete of a preset that a GPU is on reverts that GPU to
stock rather than leaving a dangling `preset:<id>` (the original "stuck on
custom" bug).

**Steps:**
1. `ID=$(mk rw-heal-test)` ; `apply $ID $A`.
2. Confirm `sources()` shows `"$A": "preset:$ID"`.
3. `del $ID` (delete via the API, as the Presets page does).
4. Wait ~1 s.

**Expected outcome:**
- The preset is deleted.
- Card A's `active_source` is healed to `stock` — it does **not** remain the
  dangling `preset:$ID`.

**How to verify:**
- `del $ID` returns `204`.
- `curl -s -o /dev/null -w '%{http_code}' localhost:8787/api/presets/$ID` → `404`.
- `sources()` → `"$A": "stock"` (not `"preset:$ID"`).

**Cleanup:** none (card A at stock).

---

## Check 5: Shared-preset propagation — editing one card updates the other

> **Superseded (2026-10-17):** a knob edit is now **transient** — it applies
> only to the editing GPU and does **not** update the shared preset, so card B
> no longer shows the new value immediately. Propagation happens when the edit
> is **Saved** (`tile-preset-save`), which writes the shared preset. The
> display-derivation behavior (tile reads the live preset, not its stale
> per-slot value) is unchanged and still holds after a Save.

**Goal:** two GPUs share a preset; editing a knob through one GPU updates the
shared preset, and the *other* GPU's tile reflects the new value immediately
(without a manual refresh). This is the "shared preset" contract: the preset is
the source of truth for every GPU on it.

**Steps:**
1. `ID=$(mk rw-share-prop)` ; `apply $ID $A` ; `apply $ID $B`.
2. Navigate to the dashboard; wait ~2 s for the preset list to load.
3. Confirm both cards' `tile-power-cap` read `210`.
4. In card A's `tile-power-cap`, type `218`, then press **Tab** (blur-commit).
5. Wait ~3 s for the apply + preset-list refresh to settle.
6. Read both cards' `tile-power-cap` again.

**Expected outcome:**
- Card A shows `218`.
- Card B **also** shows `218` — even though card B's own hardware was not
  re-applied (its per-slot applied value is still `210`). The tile derives its
  displayed value from the live shared preset, so the change propagates.

**How to verify:**
- `sources()` → both `"$A"` and `"$B"` are `preset:$ID`.
- `curl -s localhost:8787/api/presets/$ID | jq .settings.power_cap_w` → `218`.
- Per-slot applied state (the stale side): card A's
  `active_state.settings` `power_cap_w` → `218` (it was written); card B's →
  `210` (NOT written — the apply was scoped to card A). This divergence is
  expected and is exactly what the display fix papers over.
- UI (the assertion that matters): both tiles' `tile-power-cap` value == `218`.
  Before the fix, card B stayed at `210` here.

**Cleanup:** `del $ID` (reverts both cards to stock via the heal).

---

## Optional: boot heal (hard to replay via UI)

The `boot_reapply` heal (heal dangling sources on daemon start) is covered by
the Rust tests (`persist::tests::boot_reapply_*`) and by Check 4's code path.
Replaying it through the UI is not deterministic because the delete handler now
heals immediately, so a normal delete never leaves a dangling state to survive a
restart. To force it you would have to corrupt the `active_sources` meta
directly in the data dir, which is out of scope for a UI replay.

## GPU state impact / restore

Every check above changes GPU state (apply/revert) and creates `rw-*` test
presets. After the full run, restore a clean state:

```bash
# delete any leftover test presets (each reverts its GPU to stock via the heal)
for id in $(curl -s localhost:8787/api/presets | jq '.[] | select(.name|startswith("rw-")) | .id'); do
  curl -s -o /dev/null -X DELETE localhost:8787/api/presets/$id
done
# belt-and-braces: revert both cards to stock
curl -s -X POST localhost:8787/api/revert -H 'content-type: application/json' -d "{}"
```

Confirm `sources()` shows both cards at `stock` and no `rw-*` presets remain.
Leave the user's own presets untouched.

## Verdict template

- [ ] Check 1 (clone + blur-commit + label stays preset): PASS / FAIL
- [ ] Check 2 (multi-card delete blocked by red modal): PASS / FAIL
- [ ] Check 3 (solo delete → confirm → stock): PASS / FAIL
- [ ] Check 4 (API delete heals dangling source): PASS / FAIL
- [ ] Check 5 (shared-preset propagation: editing one card updates the other): PASS / FAIL
- [ ] Restore: both cards at stock, no `rw-*` presets: PASS / FAIL
