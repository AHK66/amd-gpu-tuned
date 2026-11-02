# Real-World Check: Presets panel refinement — trimmed toolbar, no knobs column, always-visible row actions

Date: 2026-10-17
Branch/commits: `feat/chart-legend-layout` at `91c5444` (this check covers
  - `03a0107` drop revert/baseline buttons from the presets toolbar
  - `bff6718` remove the knobs column from the presets table
  - `91c5444` always show the preset row actions (hover-gating broke in `hover:none` browsers))
Hardware: 4× real GPUs enumerated:
  - `1002:7551-1458:242f-0b:00.0` (on `preset:30`)
  - `1002:7551-1458:242f-0c:00.0` (on `preset:41`)
  - `1002:7551-1458:242f-44:00.0` (on `preset:41`)
  - `1002:7551-1458:242f-45:00.0` (on `preset:41`)

This entry is written as a **replayable** procedure: each check lists imperative
steps, the expected outcome, and exactly how to verify it (selector / API call /
value). Re-run it after any change to `ui/src/routes/Presets.svelte` or
`ui/src/lib/components/PresetList.svelte`.

It focuses on the three behaviors changed by this feature and re-confirms the two
that must survive them (the in-use delete guard and the per-row export).

## What is covered

Six assertions on the Presets panel:

1. **Toolbar trimmed** → exactly `new preset` + `import`; the `revert to stock`
   and `recapture baseline` buttons are gone.
2. **Knobs column removed** → the table headers are exactly
   `name | updated | actions`.
3. **Row actions always visible (the core fix)** → in a browser that reports
   `(hover: none)`, the export / duplicate / delete icons have computed
   `opacity: 1` and `pointer-events: auto` **with no hover**. This is the
   regression `91c5444` fixes: the old markup was `opacity-0 pointer-events-none`
   revealed by `group-hover:*`, which Tailwind v4.1 wraps in
   `@media (hover:hover)` — so in a `hover:none` browser the actions were
   permanently invisible and untappable.
4. **In-use delete guard survives** → deleting a preset any GPU is on is blocked
   by the "Can't delete preset" modal.
5. **Export works** → the per-row export downloads a `<name>.json` whose content
   matches the preset.
6. **Unused delete survives** → an unused preset deletes through the confirm
   dialog.

## Setup

The served UI is the `rust-embed` compile-time bundle, so rebuild both artifacts
before starting, then start the daemon (passwordless sudo — see `README.md`):

```bash
cd ui && npm run build        # -> dist/assets/index-*.js
cargo build --release         # re-embeds ui/dist
sh scripts/restart-amd-gpu-tuned.sh   # from repo root (rebuild + restart)
```

Confirm `curl -s localhost:8787/api/status` returns `ok:true`. Drive the UI with
the Playwright MCP browser tools against `http://127.0.0.1:8787/#/presets`.

> **Note on Check 3:** it requires a browser whose pointer reports
> `(hover: none)`. Headless Chromium (the Playwright MCP default) qualifies —
> verify with `matchMedia('(hover:none)').matches === true`. A touch laptop or
> tablet browser also qualifies. A desktop mouse browser reports `(hover:hover)`
> and will pass Check 3 trivially without exercising the fix.

### Helpers used throughout

- `sources()` = `curl -s localhost:8787/api/status | jq -c .active_sources`.
- `mk <name>` = `curl -s -X POST localhost:8787/api/presets -H 'content-type: application/json' -d '{"name":"<name>","settings":{"power_cap_w":200,"voltage_offset_mv":-50}}' | jq .id`.
  (Note: `voltage_offset_mv` is an `i32` — send an integer, not a float.)
- `del <id>` = `curl -s -o /dev/null -w '%{http_code}' -X DELETE localhost:8787/api/presets/<id>`.
- UI selectors: `new-preset-btn` / `import-preset-btn` (toolbar),
  `[data-testid="preset-row-<id>"]` (the row's **name button** — the row itself
  is the `tr` that contains it), `td .flex.gap-2 button[aria-label=export|duplicate|delete]`
  (row actions), `[role="dialog"]` (modal / confirm).

This check never applies a preset to a GPU, so it changes **no GPU state**; the
only mutations are creating and deleting `rw-*` test presets.

---

## Check 1: Toolbar trimmed to new preset + import

**Goal:** the toolbar carries exactly the `new preset` and `import` buttons; the
`revert to stock` and `recapture baseline` buttons no longer exist.

**Steps:**
1. Navigate to `#/presets`; wait ~2 s for the preset list to load.
2. Read the toolbar buttons and probe for the removed test ids.

**Expected outcome:**
- Toolbar buttons (in order): `new preset`, `import`.
- No element with `data-testid="revert-btn"` or `data-testid="baseline-btn"`.

**How to verify:**
- `document.querySelectorAll('.mb-3 button')` text → `["new preset","import"]`.
- `document.querySelector('[data-testid="revert-btn"]')` → `null`;
  `…('baseline-btn')` → `null`.

**Observed (2026-10-17):** `["new preset","import"]`; both removed test ids
`null`. **PASS.**

**Cleanup:** none.

---

## Check 2: Knobs column removed

**Goal:** the table headers are exactly `name | updated | actions`; there is no
`knobs` column.

**Steps:**
1. On `#/presets`, read the column headers.

**Expected outcome:**
- Headers (in order): `name`, `updated`, `actions`.
- No header whose text is `knobs`.

**How to verify:**
- `document.querySelectorAll('th')` text → `["name","updated","actions"]`.

**Observed (2026-10-17):** `["name","updated","actions"]`. **PASS.**

**Cleanup:** none.

---

## Check 3: Row actions always visible in a `hover:none` browser (core fix)

**Goal:** in a browser reporting `(hover: none)`, the export / duplicate / delete
icons are visible and clickable **without any hover** — the pre-fix markup kept
them at `opacity:0` / `pointer-events:none` because their reveal
(`group-hover:*`) is gated behind `@media (hover:hover)`.

**Steps:**
1. Confirm the browser is the failing condition:
   `matchMedia('(hover:none)').matches === true` (and
   `matchMedia('(hover:hover)').matches === false`).
2. **Do not hover.** Read the first row's actions container
   (`td .flex.gap-2`) and its three buttons.

**Expected outcome:**
- The container's class is `flex gap-2` (no `opacity-0`, no `pointer-events-none`,
  no `group-*`), computed `opacity: 1`, `pointer-events: auto`.
- Each of `export`, `duplicate`, `delete` has computed `opacity: 1` and
  `pointer-events: auto`.

**How to verify:**
- `getComputedStyle(container).opacity` == `"1"`,
  `.pointerEvents` == `"auto"`, `.className` == `"flex gap-2"`.
- For each `button[aria-label]` in the container:
  `getComputedStyle(btn).opacity` == `"1"`, `.pointerEvents` == `"auto"`.

**Observed (2026-10-17):** `hoverNone: true`, `hoverHover: false`; container
`flex gap-2` @ `opacity:1 / pointer-events:auto`; all three buttons
`opacity:1 / pointer-events:auto`. **PASS.**

> Regression probe: against the pre-`91c5444` bundle the same read returns
> container `opacity:0 / pointer-events:none` in a `hover:none` browser — this is
> the exact symptom that made the panel look "old".

**Cleanup:** none.

---

## Check 4: In-use delete guard survives

**Goal:** deleting a preset that a GPU is on is still blocked by the
"Can't delete preset" modal (this behavior predates the feature and must survive
the actions-cell rework).

**Steps:**
1. Pick a preset in use — `preset:41` (`R9700 - 225W, -105mV, 1350 MHz`), on
   three cards.
2. On its row, click the **delete** icon (no hover needed — Check 3).
3. Observe the modal; click **Dismiss**.

**Expected outcome:**
- A `role="dialog"` titled `Can't delete preset` with body
  `'R9700 - 225W, -105mV, 1350 MHz' is applied to 3 GPUs. Switch them off it
  first.` and a **Dismiss** button.
- The preset is **not** deleted.

**How to verify:**
- `[role="dialog"]` text contains `Can't delete` and `applied to 3 GPU`.
- `curl -s -o /dev/null -w '%{http_code}' localhost:8787/api/presets/41` → `200`.
- `sources()` unchanged (cards still on `preset:41`).

**Observed (2026-10-17):** modal read
`Can't delete preset … 'R9700 - 225W, -105mV, 1350 MHz' is applied to 3 GPUs.
Switch them off it first.`; Dismiss closed it; preset 41 still `200`; sources
unchanged. **PASS.**

**Cleanup:** none (preset not deleted; no GPU state changed).

---

## Check 5: Export downloads the preset as JSON

**Goal:** the per-row export (the counterpart of the toolbar import) downloads a
`<name>.json` file whose content matches the preset.

**Steps:**
1. `ID=$(mk rw-refine)`.
2. Reload `#/presets` (or wait ~5 s for the poll) so the new row appears.
3. On row `preset-row-$ID`, click **export**; capture the download.
4. Read the downloaded file.

**Expected outcome:**
- A file `rw-refine.json` is downloaded.
- Its JSON is `{"name":"rw-refine","notes":"","settings":{…,"power_cap_w":200,
  "voltage_offset_mv":-50, …null…}}` — matching the preset.

**How to verify:**
- Playwright `download.suggestedFilename()` == `rw-refine.json`.
- File content: `name` == `rw-refine`, `settings.power_cap_w` == `200`,
  `settings.voltage_offset_mv` == `-50`, other settings `null`.

**Observed (2026-10-17):** downloaded `rw-refine.json`; content
`{name:"rw-refine", settings:{power_cap_w:200, voltage_offset_mv:-50,
fan/max_mclk_mhz/sclk_offset_mhz/thermal_target_c/power_profile_mode:null}}`.
**PASS.**

**Cleanup:** handled by Check 6 (deletes `$ID`).

---

## Check 6: Unused delete survives

**Goal:** an unused preset still deletes through the `Delete preset` confirm
dialog (not the blocked modal).

**Steps:**
1. Continue from Check 5 (`$ID` = `rw-refine`, unused — never applied to a GPU).
2. On row `preset-row-$ID`, click **delete**.
3. In the `Delete preset` confirm dialog, click **delete**.

**Expected outcome:**
- A `role="dialog"` titled `Delete preset` with body `Delete "rw-refine"? This
  cannot be undone.` and **cancel** / **delete** buttons (NOT the blocked modal).
- Confirming deletes the preset.

**How to verify:**
- Confirm dialog text contains `Delete "rw-refine"?` and `This cannot be undone`.
- After confirming: `del $ID` equivalent →
  `curl -s -o /dev/null -w '%{http_code}' localhost:8787/api/presets/$ID` →
  `404`.
- `curl -s localhost:8787/api/presets | jq '[.[]|select(.name|startswith("rw-"))]|length'`
  → `0`.

**Observed (2026-10-17):** confirm dialog read
`Delete preset … Delete "rw-refine"? This cannot be undone. cancel delete`;
after confirming, preset 42 → `404`; `0` `rw-*` presets remain. **PASS.**

**Cleanup:** none (preset deleted).

---

## GPU state impact / restore

This check **never applies a preset to a GPU**, so no card's tuning changes. The
only mutations are the create + delete of `rw-*` test presets. After the run:

```bash
# delete any leftover test presets (none expected — Check 6 deletes its one)
for id in $(curl -s localhost:8787/api/presets | jq '.[] | select(.name|startswith("rw-")) | .id'); do
  curl -s -o /dev/null -X DELETE localhost:8787/api/presets/$id
done
```

Confirm `sources()` is unchanged from the start (`0b` on `preset:30`;
`0c`/`44`/`45` on `preset:41`) and no `rw-*` presets remain. The user's own
presets are untouched.

## Verdict template

- [ ] Check 1 (toolbar = new preset + import; revert/baseline gone): PASS / FAIL
- [ ] Check 2 (columns = name | updated | actions; no knobs): PASS / FAIL
- [ ] Check 3 (row actions visible + clickable, no hover, in a hover:none browser): PASS / FAIL
- [ ] Check 4 (in-use delete blocked by modal; preset survives): PASS / FAIL
- [ ] Check 5 (export downloads <name>.json matching the preset): PASS / FAIL
- [ ] Check 6 (unused delete via confirm dialog; preset 404 after): PASS / FAIL
- [ ] Restore: no `rw-*` presets, sources unchanged: PASS / FAIL
