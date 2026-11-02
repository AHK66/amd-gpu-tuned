<script lang="ts">
  import { apply, createPreset, revert, updatePreset } from "../api";
  import { Plus, Save, Clock, ExternalLink, RefreshCw, Pencil, Gauge } from "@lucide/svelte";
  import { powerHeadroom } from "../logic/power";
  import { eccCeBand, eccDeferredBand, eccUeBand, type EccInput } from "../logic/ecc";
  import { pcieBand } from "../logic/pcie";
  import { bandForMargin, BAND_CLASS, fmtGib, isThrottled, vramFreeBand } from "../logic/thermals";
  import type { Band, Sensor } from "../logic/thermals";
  import { toast } from "../store/toast.svelte";
  import { openModal, closeModal } from "../store/modal.svelte";
  import { registerTile, unregisterTile } from "../store/pending.svelte";
  import type { ApplyReport, GpuSample, GpuStatic, Preset, PresetSettings } from "../types";

  import Card from "./ui/Card.svelte";
  import Button from "./ui/Button.svelte";
  import Tooltip from "./ui/Tooltip.svelte";
  import StatusLamp from "./ui/StatusLamp.svelte";
  import Modal from "./ui/Modal.svelte";
  import { knobRange, knobTooltip } from "../logic/knobs";
  import { untrack } from "svelte";

  type Selection =
    | { kind: "default" }
    | { kind: "preset"; id: number }
    | { kind: "custom"; label: string }
    | { kind: "external" };

  type FieldKnob = "power_cap_w" | "sclk_offset_mhz" | "voltage_offset_mv" | "max_mclk_mhz";

  let {
    sample,
    ring = [],
    staticInfo,
    stale,
    presets,
    busy,
    activeSource,
    activeSettings,
    onPresetsChanged,
    onStatusChanged,
    drifted = false,
  }: {
    sample: GpuSample | null;
    ring?: GpuSample[];
    staticInfo: GpuStatic;
    stale: boolean;
    presets: Preset[];
    busy: boolean;
    activeSource: string | null;
    activeSettings: PresetSettings | null;
    onPresetsChanged: () => void;
    onStatusChanged: () => void;
    drifted?: boolean;
  } = $props();

  function deriveSelection(source: string | null, ps: Preset[]): Selection {
    if (source == null || source === "stock") return { kind: "default" };
    const m = source.match(/^preset:(\d+)$/);
    if (m) {
      const id = Number(m[1]);
      return ps.some((p) => p.id === id)
        ? { kind: "preset", id }
        : { kind: "custom", label: "custom" };
    }
    if (source === "adhoc") return { kind: "custom", label: "adhoc" };
    if (source === "watchdog") return { kind: "custom", label: "watchdog" };
    if (source === "external") return { kind: "external" };
    return { kind: "custom", label: "custom" };
  }

  let selection = $state<Selection>(untrack(() => deriveSelection(activeSource, presets)));
  // True when `selection` was derived from server state (mount / sync), false
  // when the user explicitly chose it (pick/clone/delete). The sync effect
  // writes it but no longer branches on it — a user pick is protected against
  // stale props by `optimistic` instead.
  let selectionDerived = true;
  let savedPreset = $state<Preset | null>(null);
  // A just-made pick, held until the server confirms it. `source` is the
  // activeSource the apply will produce ("preset:<id>" / "stock"); `base` is
  // the activeSource at pick time (a poll still showing `base` is stale, not
  // a change); `at` bounds the hold to 5 s against degenerate same-source
  // changes (e.g. external→external) landing mid-apply.
  let optimistic = $state<{ source: string; base: string | null; at: number } | null>(null);
  let comboText = $state("");
  let listOpen = $state(false);
  let renaming = $state(false);
  let comboEl: HTMLInputElement | undefined = $state();
  let comboRoot: HTMLDivElement | undefined = $state();
  let saving = $state(false);

  const disabled = $derived(busy || saving);

  function stockCap(): number | null {
    return staticInfo.cap_default_w ?? staticInfo.cap_max_w;
  }
  function stockMclk(): number | null {
    if (staticInfo.dpm_mclk_levels.length > 0) return Math.max(...staticInfo.dpm_mclk_levels);
    return sample?.dpm_mclk_mhz ?? null;
  }
  // The preset the server says this GPU is on, keyed off `activeSource` (the
  // server's source of truth) rather than the optimistic `selection` — the
  // selection-sync effect never downgrades a valid preset, so `selection` can
  // transiently point at a preset even when `activeSource` is stock/adhoc.
  const sourcePreset = $derived.by(() => {
    if (typeof activeSource !== "string" || !activeSource.startsWith("preset:")) return null;
    const id = Number(activeSource.slice("preset:".length));
    return presets.find((p) => p.id === id) ?? null;
  });
  // Display base for each knob: what this card is actually running. The
  // per-slot applied state (`activeSettings`) is the source of truth — it
  // records the merged preset+override values the last apply wrote to THIS
  // slot, so a transient field edit (applied to the card but not persisted to
  // the preset) survives a UI reload. Fall back to the live preset settings
  // when the slot has no applied state yet, and to stock values when neither
  // exists (stock/adhoc slots have no preset object at all).
  const effective = $derived({
    power_cap_w: (activeSettings ?? sourcePreset?.settings ?? {}).power_cap_w ?? stockCap(),
    sclk_offset_mhz: (activeSettings ?? sourcePreset?.settings ?? {}).sclk_offset_mhz ?? 0,
    voltage_offset_mv: (activeSettings ?? sourcePreset?.settings ?? {}).voltage_offset_mv ?? 0,
    max_mclk_mhz: (activeSettings ?? sourcePreset?.settings ?? {}).max_mclk_mhz ?? stockMclk(),
  });

  function str(n: number | null): string {
    return n == null ? "" : String(n);
  }

  function num(s: string): number | null {
    const t = s.trim();
    if (t === "") return null;
    const n = Number(t);
    return Number.isFinite(n) ? n : null;
  }

  let fieldError = $state<Partial<Record<FieldKnob, string>>>({});

  function validateField(knob: FieldKnob, raw: string): string | null {
    const value = num(raw);
    if (value == null) return null;
    const range = knobRange(knob, staticInfo);
    if (range == null) return null;
    if (value < range[0] || value > range[1]) {
      return `range: ${range[0]} … ${range[1]}`;
    }
    return null;
  }

  let draft = $state<Partial<Record<FieldKnob, string>>>({});
  let local = $state<Partial<Record<FieldKnob, number | null>>>({});
  let expected = $state<{
    knob: FieldKnob;
    value: number | null;
    source: string;
    baseSource: string | null;
    baseSettings: PresetSettings | null;
  } | null>(null);

  function displayValue(knob: FieldKnob): string {
    if (draft[knob] !== undefined) return draft[knob];
    if (local[knob] !== undefined) return str(local[knob]);
    return str(effective[knob]);
  }

  function clearOverlays() {
    draft = {};
    local = {};
    fieldError = {};
    expected = null;
  }

  function sameValues(a: PresetSettings | null, b: PresetSettings | null): boolean {
    return (
      (a?.power_cap_w ?? null) === (b?.power_cap_w ?? null) &&
      (a?.sclk_offset_mhz ?? null) === (b?.sclk_offset_mhz ?? null) &&
      (a?.voltage_offset_mv ?? null) === (b?.voltage_offset_mv ?? null) &&
      (a?.max_mclk_mhz ?? null) === (b?.max_mclk_mhz ?? null)
    );
  }

  function sameSelection(a: Selection, b: Selection): boolean {
    if (a.kind !== b.kind) return false;
    if (a.kind === "preset" && b.kind === "preset") return a.id === b.id;
    if (a.kind === "custom" && b.kind === "custom") return a.label === b.label;
    return true;
  }

  // Plain `let`, NOT $state: the effect reads and writes it, and a reactive
  // dependency on its own write would loop the effect.
  let prevProps = untrack(() => ({ source: activeSource, settings: activeSettings }));
  $effect(() => {
    const src = activeSource;
    const ps = activeSettings;
    const exp = expected;
    const changed = src !== prevProps.source || !sameValues(ps, prevProps.settings);
    prevProps = { source: src, settings: ps };
    if (!changed) return;
    if (exp == null) {
      clearOverlays(); // external change with nothing pending
      return;
    }
    const caughtUp = src === exp.source && (ps?.[exp.knob] ?? null) === exp.value;
    const stale = src === exp.baseSource && sameValues(ps, exp.baseSettings);
    if (caughtUp) {
      // Keep `local`: with transient edits the per-slot applied state catches
      // up, but the SAVED preset does not — clearing the overlay would snap the
      // field back to the saved value. `local` is the transient divergence that
      // `modified` compares and Save persists.
      expected = null;
    } else if (!stale) {
      clearOverlays(); // an external write won
    }
    // else: props still stale — keep waiting
  });

  function currentSettings(edit?: { knob: FieldKnob; value: number | null }): PresetSettings {
    const v = (knob: FieldKnob) =>
      edit?.knob === knob ? edit.value : local[knob] !== undefined ? local[knob] : effective[knob];
    return {
      power_cap_w: v("power_cap_w"),
      sclk_offset_mhz: v("sclk_offset_mhz"),
      max_mclk_mhz: v("max_mclk_mhz"),
      voltage_offset_mv: v("voltage_offset_mv"),
    };
  }

  async function run(fn: () => Promise<ApplyReport>, okMsg: string): Promise<boolean> {
    saving = true;
    try {
      const r = await fn();
      if (r.ok) {
        toast.success(okMsg);
        onStatusChanged();
        return true;
      }
      const failed = (r.per_gpu[staticInfo.slot] ?? []).filter((o) => o.status === "failed");
      const detail =
        failed.length > 0 ? failed.map((o) => `${o.field}: ${o.detail}`).join(", ") : "unknown error";
      toast.error(`apply failed: ${detail}`);
      onStatusChanged();
      return false;
    } catch (e) {
      toast.error(`apply failed: ${String(e)}`);
      return false;
    } finally {
      saving = false;
    }
  }

  const activePreset = $derived.by(() => {
    if (selection.kind !== "preset") return null;
    const id = selection.id;
    return presets.find((p) => p.id === id) ?? null;
  });
  const editable = $derived(
    selection.kind === "preset" && activePreset != null && !activePreset.is_builtin,
  );
  const fieldDisabled = $derived(disabled || !editable);

  const TILE_KNOBS: FieldKnob[] = [
    "power_cap_w",
    "sclk_offset_mhz",
    "voltage_offset_mv",
    "max_mclk_mhz",
  ];
  // "Unsaved" means: differs from the SAVED preset's settings — not from
  // `effective`, which catches up to a committed edit as soon as the per-slot
  // applied state lands (that would drop the Save affordance right after the
  // apply succeeds).
  const modified = $derived(
    selection.kind === "preset" &&
      activePreset != null &&
      !activePreset.is_builtin &&
      TILE_KNOBS.some((k) => local[k] != null && local[k] !== activePreset.settings[k]),
  );

  function diverged(knob: FieldKnob): boolean {
    return (
      selection.kind === "preset" &&
      activePreset != null &&
      !activePreset.is_builtin &&
      local[knob] != null &&
      local[knob] !== activePreset.settings[knob]
    );
  }

  // Re-derive the transient divergence from server state: the per-slot applied
  // settings (`activeSettings`) record what was last written to this card, so
  // a transient edit from a previous session re-activates `modified`, the Save
  // button, and the unsaved-change guards after a reload. Only knobs not
  // already held in `local` are seeded, and only while the slot runs a
  // non-builtin preset. `saved !== null` is required: a commit re-merges the
  // preset's concrete values for ALL knobs (currentSettings fills unset knobs
  // from `effective`), so a knob the preset leaves unset has `saved == null`
  // and its concrete applied value (0 / stock) is NOT a divergence.
  $effect(() => {
    const ps = activeSettings;
    const ap = activePreset;
    if (ps == null || ap == null || ap.is_builtin) return;
    const cur = untrack(() => local);
    for (const k of TILE_KNOBS) {
      if (cur[k] === undefined) {
        const applied = ps[k] ?? null;
        const saved = ap.settings[k] ?? null;
        if (applied !== null && saved !== null && applied !== saved) local[k] = applied;
      }
    }
  });

  // `selection` is a cache of the server's active state: while no user apply
  // is in flight it follows `deriveSelection(activeSource, presets)` — an
  // external adoption, a stock revert, a second client's preset apply, and a
  // deleted preset all update the label without a reload. A just-made pick is
  // held (via `optimistic`) until the server source confirms it, which covers
  // the window after `saving` clears but before the post-apply status fetch
  // lands — a `saving`-only guard would snap the label back to the stale
  // source on every successful pick.
  $effect(() => {
    if (saving) return; // apply request in flight: optimistic selection wins
    if (optimistic != null) {
      if (activeSource === optimistic.source || Date.now() - optimistic.at > 5000) {
        optimistic = null; // confirmed (or safety-bound expiry)
      } else if (activeSource !== optimistic.base) {
        optimistic = null; // a real change won
      } else {
        return; // stale poll: hold the optimistic selection
      }
    }
    const derived = deriveSelection(activeSource, presets);
    if (sameSelection(selection, derived)) return;
    if (selection.kind === "preset" && derived.kind !== "preset") {
      const id = selection.id;
      if (savedPreset != null && savedPreset.id === id && !presets.some((p) => p.id === id)) {
        return; // just-cloned preset not in the (not yet refetched) list
      }
      savedPreset = null;
      clearOverlays(); // an external/stock write won: unsaved edits are gone
    }
    selection = derived;
    selectionDerived = true;
  });

  function activeName(): string {
    switch (selection.kind) {
      case "default":
        return "stock";
      case "preset":
        return activePreset?.name ?? "custom";
      case "custom":
        return selection.label;
      case "external":
        return "modified outside amd-gpu-tuned";
    }
  }
  // Register with the pending store exactly while `modified`, so the
  // leave-Dashboard guard sees this tile. The cleanup unregisters on effect
  // re-run *and* on component destroy, so a tile that unmounts while
  // `modified` (e.g. the GPU disappears from status) can't leave a stale entry.
  $effect(() => {
    if (modified)
      registerTile(staticInfo.slot, {
        label: activeName(),
        save: saveCurrent,
        discard: discardCurrent,
      });
    else unregisterTile(staticInfo.slot);
    return () => unregisterTile(staticInfo.slot);
  });

  const comboLabel = $derived(activeName());
  $effect(() => {
    if (renaming) return;
    if (comboEl != null && document.activeElement === comboEl) return;
    comboText = comboLabel;
  });

  let pendingPick = $state<Selection | null>(null);
  const switchModalId = $derived(`gpu-tile-switch-${staticInfo.slot}`);

  function doPick(sel: Selection) {
    savedPreset = null;
    clearOverlays();
    optimistic = {
      // Inverted on purpose: in the falsy branch `sel` would still include
      // `custom`/`external`, which have no `id` (only `default`/`preset`
      // reach doPick in practice).
      source: sel.kind === "preset" ? `preset:${sel.id}` : "stock",
      base: activeSource,
      at: Date.now(),
    };
    selection = sel;
    selectionDerived = false;
    if (sel.kind === "default")
      void run(() => revert({ gpu: staticInfo.slot }), "reverted to stock").then((ok) => {
        if (!ok) optimistic = null;
      });
    else if (sel.kind === "preset")
      void run(() => apply({ gpu: staticInfo.slot, preset_id: sel.id }), "preset applied").then(
        (ok) => {
          if (!ok) optimistic = null;
        },
      );
  }

  function pick(sel: Selection) {
    listOpen = false;
    if (sel.kind === "default" && selection.kind === "default") return;
    if (sel.kind === "preset" && selection.kind === "preset" && sel.id === selection.id) return;
    if (modified) {
      pendingPick = sel;
      openModal(switchModalId);
      return;
    }
    doPick(sel);
  }

  async function switchSave() {
    closeModal();
    const ok = await saveCurrent();
    if (ok && pendingPick) doPick(pendingPick);
    pendingPick = null;
  }

  async function switchDismiss() {
    closeModal();
    const ok = await discardCurrent();
    if (ok && pendingPick) doPick(pendingPick);
    pendingPick = null;
  }

  function switchStay() {
    closeModal();
    pendingPick = null;
  }

  function onComboClick(e: MouseEvent) {
    if (renaming) return;
    if (listOpen) {
      if (!editable) return;
      renaming = true;
      const x = e.clientX;
      requestAnimationFrame(() => {
        const el = comboEl;
        if (!el) return;
        el.focus();
        const idx = caretIndexFromClick(el, x);
        el.setSelectionRange(idx, idx);
      });
    } else {
      listOpen = true;
    }
  }

  // Map a click's X position to a caret index in a single-line <input>, using a
  // hidden mirror span with identical font (no native API exists for <input>).
  function caretIndexFromClick(input: HTMLInputElement, clientX: number): number {
    const text = input.value;
    if (text.length === 0) return 0;
    const style = getComputedStyle(input);
    const mirror = document.createElement("span");
    mirror.style.position = "absolute";
    mirror.style.visibility = "hidden";
    mirror.style.whiteSpace = "pre";
    mirror.style.font = style.font;
    mirror.style.letterSpacing = style.letterSpacing;
    document.body.appendChild(mirror);
    const rect = input.getBoundingClientRect();
    const contentLeft =
      rect.left + parseFloat(style.borderLeftWidth) + parseFloat(style.paddingLeft);
    const targetX = Math.max(0, clientX - contentLeft);
    let lo = 0;
    let hi = text.length;
    while (lo < hi) {
      const mid = (lo + hi) >> 1;
      mirror.textContent = text.slice(0, mid);
      if (mirror.getBoundingClientRect().width < targetX) lo = mid + 1;
      else hi = mid;
    }
    document.body.removeChild(mirror);
    return lo;
  }

  function commitRename() {
    if (!renaming) return;
    renaming = false;
    const t = comboText.trim();
    if (t === "" || t === activeName()) return;
    if (selection.kind === "preset" && activePreset != null && !activePreset.is_builtin) {
      void applyRename(t);
    }
  }

  async function applyRename(name: string): Promise<void> {
    if (selection.kind !== "preset") return;
    saving = true;
    try {
      const p = await updatePreset(selection.id, { name });
      savedPreset = p;
      onPresetsChanged();
    } catch (e) {
      toast.error(String(e));
    } finally {
      saving = false;
    }
  }

  function cancelRename() {
    if (!renaming) return;
    renaming = false;
    comboText = comboLabel;
  }

  function commitField(knob: FieldKnob) {
    const raw = draft[knob] ?? displayValue(knob);
    const value = num(raw);
    const err = validateField(knob, raw);
    if (err) {
      fieldError[knob] = err;
      return; // keep the typed value; the inline error is the feedback
    }
    fieldError[knob] = undefined;
    draft[knob] = undefined;
    local[knob] = value;
    // Apply *through* the preset id so the GPU's active_source stays
    // `preset:<id>` (an adhoc apply would relabel it "adhoc"). The value is
    // transient: it is NOT persisted to the preset here (see saveCurrent).
    const target = selection.kind === "preset" ? selection.id : null;
    const source = target != null ? `preset:${target}` : "adhoc";
    expected = {
      knob,
      value,
      source,
      baseSource: activeSource,
      baseSettings: activeSettings,
    };
    void run(
      () =>
        target != null
          ? apply({
              gpu: staticInfo.slot,
              preset_id: target,
              overrides: currentSettings({ knob, value }),
            })
          : apply({ gpu: staticInfo.slot, overrides: currentSettings({ knob, value }) }),
      "applied",
    ).then((ok) => {
      if (!ok) {
        local[knob] = undefined;
        expected = null;
      }
    });
  }

  // Committing on blur (not just Enter) so a typed value is never silently
  // discarded when the user clicks away — that read as "my value got reverted".
  //   - untouched field (no draft)  → just clear any stale inline error;
  //   - valid draft                  → commit it (same as pressing Enter);
  //   - invalid draft                → discard it, reverting to the effective
  //     value (an out-of-range number can't be applied).
  function onFieldBlur(knob: FieldKnob) {
    const raw = draft[knob];
    if (raw === undefined) {
      fieldError[knob] = undefined;
      return;
    }
    if (validateField(knob, raw) != null) {
      draft[knob] = undefined;
      fieldError[knob] = undefined;
    } else {
      commitField(knob);
    }
  }

  async function saveCurrent(): Promise<boolean> {
    if (selection.kind !== "preset" || activePreset == null) return false;
    const id = selection.id;
    const settings: PresetSettings = {
      ...activePreset.settings,
      power_cap_w: local.power_cap_w ?? effective.power_cap_w,
      sclk_offset_mhz: local.sclk_offset_mhz ?? effective.sclk_offset_mhz,
      max_mclk_mhz: local.max_mclk_mhz ?? effective.max_mclk_mhz,
      voltage_offset_mv: local.voltage_offset_mv ?? effective.voltage_offset_mv,
    };
    saving = true;
    try {
      const p = await updatePreset(id, { settings });
      savedPreset = p;
      local = {};
      expected = null;
      onPresetsChanged();
      return true;
    } catch (e) {
      toast.error(String(e));
      return false;
    } finally {
      saving = false;
    }
  }

  async function discardCurrent(): Promise<boolean> {
    if (selection.kind !== "preset") return false;
    const id = selection.id;
    const ok = await run(() => apply({ gpu: staticInfo.slot, preset_id: id }), "changes discarded");
    if (ok) {
      local = {};
      expected = null;
    }
    return ok;
  }

  async function cloneCurrent(): Promise<void> {
    if (disabled) return;
    const base = activeName();
    let n = 1;
    while (presets.some((p) => p.name === `${base} (${n})`)) n++;
    const name = `${base} (${n})`;
    saving = true;
    try {
      const p = await createPreset({ name, settings: currentSettings() });
      savedPreset = p;
      selection = { kind: "preset", id: p.id as number };
      selectionDerived = false;
      onPresetsChanged();
      await run(() => apply({ gpu: staticInfo.slot, preset_id: p.id as number }), "preset created");
    } catch (e) {
      toast.error(String(e));
      onPresetsChanged();
    } finally {
      saving = false;
    }
  }

  function onDocPointerDown(e: PointerEvent) {
    if (listOpen && comboRoot != null && !e.composedPath().includes(comboRoot)) listOpen = false;
  }
  $effect(() => {
    document.addEventListener("pointerdown", onDocPointerDown);
    return () => document.removeEventListener("pointerdown", onDocPointerDown);
  });

  const headroom = $derived(sample ? powerHeadroom(sample) : null);
  const throttled = $derived(sample ? isThrottled(sample) : false);
  const vramFreeMb = $derived(
    sample && sample.vram_used_mb != null && sample.vram_total_mb != null
      ? Math.max(0, sample.vram_total_mb - sample.vram_used_mb)
      : null,
  );

  function sensorBand(sensor: Sensor): Band | null {
    if (!sample) return null;
    const temp = sensor === "edge" ? sample.edge_c : sensor === "hotspot" ? sample.hotspot_c : sample.vram_c;
    const threshold =
      sensor === "edge"
        ? staticInfo.slowdown_edge_c
        : sensor === "hotspot"
          ? staticInfo.slowdown_hotspot_c
          : staticInfo.slowdown_vram_c;
    if (temp == null || threshold == null) return null;
    return bandForMargin(threshold - temp);
  }

  const ceNewInWindow = $derived.by(() => {
    const r = ring;
    if (r.length < 2) return false;
    if (!r.every((s) => s.ecc_umc_ce != null)) return false;
    const oldest = r[0]!.ecc_umc_ce;
    const newest = r[r.length - 1]!.ecc_umc_ce;
    return newest! - oldest! > 0;
  });

  const eccInput = $derived.by((): EccInput | null => {
    if (!sample) return null;
    if (
      sample.ecc_umc_ce == null &&
      sample.ecc_umc_ue == null &&
      sample.ecc_umc_deferred == null
    )
      return null;
    return {
      ce: sample.ecc_umc_ce,
      ue: sample.ecc_umc_ue,
      deferred: sample.ecc_umc_deferred,
      ceNewInWindow,
    };
  });

  const ceBand = $derived(eccInput != null ? eccCeBand(eccInput) : null);
  const ueBand = $derived(eccInput != null ? eccUeBand(eccInput) : null);
  const dBand = $derived(eccInput != null ? eccDeferredBand(eccInput) : null);

  const eccTitle = $derived(
    eccInput
      ? `VRAM ECC — correctable: ${eccInput.ce ?? 0} · uncorrectable: ${eccInput.ue ?? 0} · deferred: ${eccInput.deferred ?? "—"} (cumulative since last driver reset)`
      : "VRAM ECC data unavailable",
  );
  const pcieHazard = $derived(sample ? pcieBand(sample.pcie_width, sample.pcie_speed_gt) : "ok");
</script>

<Card data-testid="gpu-tile">
  <div class="flex min-h-[22px] items-center justify-between">
    <span class="font-mono text-xs whitespace-nowrap text-faint">{staticInfo.slot}</span>
    <span class="flex shrink-0 items-center gap-1.5">
      {#if stale}
        <StatusLamp icon={Clock} tone="warn" testid="tile-lamp-stale" text="No fresh telemetry (link stale)" />
      {/if}
      {#if selection.kind === "external"}
        <StatusLamp icon={ExternalLink} tone="warn" testid="tile-lamp-outside" text="Settings changed outside amd-gpu-tuned" />
      {/if}
      {#if drifted}
        <StatusLamp icon={RefreshCw} tone="info" testid="tile-lamp-drifted" text="Value drifted — auto-restoring" />
      {/if}
      {#if modified}
        <StatusLamp icon={Pencil} tone="warn" testid="tile-lamp-unsaved" text="Differs from saved preset — not saved" />
      {/if}
      {#if throttled}
        <StatusLamp icon={Gauge} tone="danger" testid="tile-lamp-throttled" text="GPU is throttling (thermal / power)" />
      {/if}
    </span>
  </div>

  {#if sample}
    <div class="mt-2 flex items-baseline gap-2">
      <span class="text-2xl text-text">{Math.round(sample.socket_power_w)} W</span>
      {#if headroom}
        <span class="text-xs text-faint">
          cap {Math.round(sample.power_cap_w ?? 0)} W · {Math.round(headroom.headroom_w)} W headroom
        </span>
      {/if}
    </div>
    {#if headroom}
      <div class="mt-1 h-1.5 w-full rounded-pill bg-surface-2">
        <div class="h-1.5 rounded-pill bg-info" style="width: {headroom.used_pct}%"></div>
      </div>
    {/if}

    <div class="mt-3 grid grid-cols-[1fr_1fr_1.4fr] items-baseline gap-2 text-xs" data-testid="tile-sensors">
      <div>edge {#if sample.edge_c != null}<span class={sensorBand("edge") ? BAND_CLASS[sensorBand("edge")!] : ""}>{sample.edge_c}°C</span>{:else}—{/if}</div>
      <div>hotspot {#if sample.hotspot_c != null}<span class={sensorBand("hotspot") ? BAND_CLASS[sensorBand("hotspot")!] : ""}>{sample.hotspot_c}°C</span>{:else}—{/if}</div>
      <div class="whitespace-nowrap">vram {#if sample.vram_c != null}<span class={sensorBand("vram") ? BAND_CLASS[sensorBand("vram")!] : ""}>{sample.vram_c}°C</span>{:else}—{/if}{#if vramFreeMb != null}<span class="text-faint">/</span><span class={BAND_CLASS[vramFreeBand(vramFreeMb, sample.vram_total_mb!)]}>{fmtGib(vramFreeMb)}</span><span class="text-[11px] text-faint">&nbsp;free</span>{/if}</div>
    </div>
    <div class="mt-1 grid grid-cols-[1fr_1fr_1.4fr] items-baseline gap-2 text-xs" data-testid="tile-ecc">
      <div title={eccTitle}>
        {#if eccInput}
          <span class="text-faint">ECC</span>
          <span class={ceBand ? BAND_CLASS[ceBand] : ""}>{sample.ecc_umc_ce ?? 0}c</span>
          <span class="text-faint">·</span>
          <span class={ueBand ? BAND_CLASS[ueBand] : ""}>{sample.ecc_umc_ue ?? 0}u</span>
          {#if sample.ecc_umc_deferred != null}
            <span class="text-faint">·</span>
            <span class={dBand ? BAND_CLASS[dBand] : ""}>{sample.ecc_umc_deferred}d</span>
          {/if}
        {:else}
          <span class="text-faint">ECC —</span>
        {/if}
      </div>
      <div>
        {#if sample.pcie_width != null && sample.pcie_speed_gt != null}
          <span class="text-faint">PCIe</span>
          <span class={BAND_CLASS[pcieHazard]}>x{sample.pcie_width} · {sample.pcie_speed_gt!.toFixed(1)} GT/s</span>
        {:else}
          <span class="text-faint">PCIe —</span>
        {/if}
      </div>
      <div></div>
    </div>
  {:else}
    <p class="mt-2 text-xs text-faint">no telemetry yet</p>
  {/if}

  <div class="mt-3 border-t border-hairline pt-3" data-testid="tile-controls">
    <div class="grid grid-cols-2 gap-2">
      <div class="text-xs">
        <Tooltip text={knobTooltip("power_cap_w", staticInfo)} class="w-full">
          <div class="flex w-full flex-col">
            <span class="flex items-center gap-1 text-faint">
              power cap W
              {#if diverged("power_cap_w")}
                <span data-testid="tile-diverged-power-cap" class="h-1.5 w-1.5 rounded-full bg-warn" aria-label="diverged from saved preset"></span>
              {/if}
            </span>
            <input type="text" inputmode="decimal" class="mt-0.5 w-full rounded-md border {fieldError.power_cap_w ? 'border-danger' : diverged('power_cap_w') ? 'border-warn' : 'border-hairline'} bg-surface-2 px-2 py-1 text-sm text-text" value={displayValue("power_cap_w")} disabled={fieldDisabled} oninput={(e) => (draft.power_cap_w = e.currentTarget.value)} onkeydown={(e) => { if (e.key === "Enter") { e.preventDefault(); commitField("power_cap_w"); } }} onblur={() => onFieldBlur("power_cap_w")} data-testid="tile-power-cap" />
            {#if fieldError.power_cap_w}
              <span class="mt-0.5 text-xs text-danger">{fieldError.power_cap_w}</span>
            {/if}
          </div>
        </Tooltip>
      </div>
      <div class="text-xs">
        <Tooltip text={knobTooltip("sclk_offset_mhz", staticInfo)} class="w-full">
          <div class="flex w-full flex-col">
            <span class="flex items-center gap-1 text-faint">
              sclk offset MHz
              {#if diverged("sclk_offset_mhz")}
                <span data-testid="tile-diverged-sclk" class="h-1.5 w-1.5 rounded-full bg-warn" aria-label="diverged from saved preset"></span>
              {/if}
            </span>
            <input type="text" inputmode="decimal" class="mt-0.5 w-full rounded-md border {fieldError.sclk_offset_mhz ? 'border-danger' : diverged('sclk_offset_mhz') ? 'border-warn' : 'border-hairline'} bg-surface-2 px-2 py-1 text-sm text-text" value={displayValue("sclk_offset_mhz")} disabled={fieldDisabled} oninput={(e) => (draft.sclk_offset_mhz = e.currentTarget.value)} onkeydown={(e) => { if (e.key === "Enter") { e.preventDefault(); commitField("sclk_offset_mhz"); } }} onblur={() => onFieldBlur("sclk_offset_mhz")} data-testid="tile-sclk" />
            {#if fieldError.sclk_offset_mhz}
              <span class="mt-0.5 text-xs text-danger">{fieldError.sclk_offset_mhz}</span>
            {/if}
          </div>
        </Tooltip>
      </div>
      <div class="text-xs">
        <Tooltip text={knobTooltip("voltage_offset_mv", staticInfo)} class="w-full">
          <div class="flex w-full flex-col">
            <span class="flex items-center gap-1 text-faint">
              voltage offset mV
              {#if diverged("voltage_offset_mv")}
                <span data-testid="tile-diverged-voltage" class="h-1.5 w-1.5 rounded-full bg-warn" aria-label="diverged from saved preset"></span>
              {/if}
            </span>
            <input type="text" inputmode="decimal" class="mt-0.5 w-full rounded-md border {fieldError.voltage_offset_mv ? 'border-danger' : diverged('voltage_offset_mv') ? 'border-warn' : 'border-hairline'} bg-surface-2 px-2 py-1 text-sm text-text" value={displayValue("voltage_offset_mv")} disabled={fieldDisabled} oninput={(e) => (draft.voltage_offset_mv = e.currentTarget.value)} onkeydown={(e) => { if (e.key === "Enter") { e.preventDefault(); commitField("voltage_offset_mv"); } }} onblur={() => onFieldBlur("voltage_offset_mv")} data-testid="tile-voltage" />
            {#if fieldError.voltage_offset_mv}
              <span class="mt-0.5 text-xs text-danger">{fieldError.voltage_offset_mv}</span>
            {/if}
          </div>
        </Tooltip>
      </div>
      <div class="text-xs">
        <Tooltip text={knobTooltip("max_mclk_mhz", staticInfo)} class="w-full">
          <div class="flex w-full flex-col">
            <span class="flex items-center gap-1 text-faint">
              max mclk MHz
              {#if diverged("max_mclk_mhz")}
                <span data-testid="tile-diverged-mclk" class="h-1.5 w-1.5 rounded-full bg-warn" aria-label="diverged from saved preset"></span>
              {/if}
            </span>
            <input type="text" inputmode="decimal" class="mt-0.5 w-full rounded-md border {fieldError.max_mclk_mhz ? 'border-danger' : diverged('max_mclk_mhz') ? 'border-warn' : 'border-hairline'} bg-surface-2 px-2 py-1 text-sm text-text" value={displayValue("max_mclk_mhz")} disabled={fieldDisabled} oninput={(e) => (draft.max_mclk_mhz = e.currentTarget.value)} onkeydown={(e) => { if (e.key === "Enter") { e.preventDefault(); commitField("max_mclk_mhz"); } }} onblur={() => onFieldBlur("max_mclk_mhz")} data-testid="tile-mclk" />
            {#if fieldError.max_mclk_mhz}
              <span class="mt-0.5 text-xs text-danger">{fieldError.max_mclk_mhz}</span>
            {/if}
          </div>
        </Tooltip>
      </div>
    </div>
    <div class="mt-2 flex items-center gap-2">
      <Button
        variant="ghost"
        size="sm"
        icon={Plus}
        aria-label="New preset"
        title="New preset (clone of current)"
        data-testid="tile-preset-add"
        {disabled}
        onclick={cloneCurrent}
        class="shrink-0"
      >
        <span class="sr-only">new preset</span>
      </Button>
      <div class="relative min-w-0 flex-1" bind:this={comboRoot}>
        <input
          type="text"
          class="w-full truncate rounded-md border border-hairline bg-surface-2 px-2 py-1 text-xs text-text"
          bind:this={comboEl}
          value={comboText}
          readonly={!renaming}
          {disabled}
          data-testid="tile-preset"
          onclick={(e) => onComboClick(e)}
          oninput={(e) => {
            if (renaming) comboText = e.currentTarget.value;
          }}
          onblur={commitRename}
          onkeydown={(e) => {
            if (renaming) {
              if (e.key === "Enter") {
                e.preventDefault();
                commitRename();
                listOpen = false;
                comboEl?.blur();
              } else if (e.key === "Escape") {
                e.preventDefault();
                cancelRename();
                listOpen = false;
              }
            } else if (e.key === "ArrowDown") {
              listOpen = true;
            }
          }}
        />
        {#if listOpen}
          <ul class="absolute z-10 mt-1 max-h-48 w-full overflow-auto rounded-md border border-hairline bg-surface-2 py-1 text-xs" data-testid="tile-preset-list">
            <li>
                <button type="button" class="block w-full px-2 py-1 text-left text-text hover:opacity-80" data-testid="tile-preset-opt-default" onclick={() => pick({ kind: "default" })}>stock <span class="text-[11px] text-faint">as-shipped settings</span></button>
            </li>
            {#each presets as p (p.id)}
              <li>
                <button type="button" class="block w-full px-2 py-1 text-left text-text hover:opacity-80" data-testid="tile-preset-opt-{p.id}" onclick={() => pick({ kind: "preset", id: p.id as number })}>{p.name} {#if p.is_builtin}<span class="text-[11px] text-faint">(built-in)</span>{/if}</button>
              </li>
            {/each}
          </ul>
        {/if}
      </div>
      <Button
        variant="ghost"
        size="sm"
        icon={Save}
        aria-label="Save preset"
        title="Save preset"
        data-testid="tile-preset-save"
        disabled={disabled || !modified}
        onclick={saveCurrent}
        class="shrink-0"
      >
        <span class="sr-only">save preset</span>
      </Button>
    </div>
  </div>
  <Modal id={switchModalId} title="Unsaved changes">
    <p class="text-muted">
      You have unsaved changes to '{activeName()}'. Save them before switching?
    </p>
    <div class="mt-4 flex justify-end gap-2">
      <Button variant="ghost" data-testid="tile-switch-stay" disabled={saving} onclick={switchStay}>Stay</Button>
      <Button variant="danger" data-testid="tile-switch-dismiss" disabled={saving} onclick={switchDismiss}>Dismiss</Button>
      <Button variant="primary" data-testid="tile-switch-save" disabled={saving} onclick={switchSave}>Save</Button>
    </div>
  </Modal>
</Card>
