<script lang="ts">
  import {
    createPreset,
    deletePreset,
    exportPreset,
    getStatus,
    listPresets,
    updatePreset,
  } from "../lib/api";
  import PresetEditor from "../lib/components/PresetEditor.svelte";
  import PresetList from "../lib/components/PresetList.svelte";
  import Button from "../lib/components/ui/Button.svelte";
  import ConfirmDialog from "../lib/components/ui/ConfirmDialog.svelte";
  import Modal from "../lib/components/ui/Modal.svelte";
  import { emptySlotForm, settingsFromSlotForm, slotFormFromSettings } from "../lib/logic/form";
  import { firstViolation, violationMessage, type KnobId } from "../lib/logic/knobs";
  import { toast } from "../lib/store/toast.svelte";
  import { openModal, closeModal } from "../lib/store/modal.svelte";
  import { Plus } from "@lucide/svelte";
  import type { SlotForm } from "../lib/logic/form";
  import type { GpuStatic, Preset, Status } from "../lib/types";

  let presets = $state<Preset[]>([]);
  let status = $state<Status | null>(null);
  let mode = $state<"list" | "edit">("list");
  let selectedId = $state<number | null>(null);
  let name = $state("");
  let notes = $state("");
  let form = $state<SlotForm>(emptySlotForm());
  let pendingDelete = $state<Preset | null>(null);

  const isBuiltin = $derived(
    (mode === "edit" && presets.find((p) => p.id === selectedId)?.is_builtin) ??
      false,
  );
  const firstGpu = $derived(status?.gpus[0] ?? null);

  const emptyStatic: GpuStatic = {
    slot: "",
    card_n: 0,
    vbios: null,
    slowdown_edge_c: null,
    slowdown_hotspot_c: null,
    slowdown_vram_c: null,
    cap_min_w: null,
    cap_max_w: null,
    cap_default_w: null,
    dpm_sclk_levels: [],
    dpm_mclk_levels: [],
    overdrive: false,
    sclk_offset_range_mhz: null,
    mclk_range_mhz: null,
    voltage_offset_range_mv: null,
    pmfw_fan: false,
    thermal_target: false,
    power_profile: false,
  };

  $effect(() => {
    const tick = () => {
      listPresets().then((p) => (presets = p)).catch((e) => toast.error(String(e)));
      getStatus().then((s) => (status = s)).catch(() => {});
    };
    tick();
    const t = setInterval(tick, 5000);
    return () => clearInterval(t);
  });

  function startNew() {
    name = "";
    notes = "";
    selectedId = null;
    form = emptySlotForm();
    mode = "edit";
  }

  function startEdit(p: Preset) {
    name = p.name;
    notes = p.notes;
    selectedId = p.id;
    form = slotFormFromSettings(p.settings);
    mode = "edit";
  }

  async function save() {
    if (isBuiltin) return;
    const settings = settingsFromSlotForm(form);
    const v = firstViolation(
      {
        power_cap_w: settings.power_cap_w,
        sclk_offset_mhz: settings.sclk_offset_mhz,
        max_mclk_mhz: settings.max_mclk_mhz,
        voltage_offset_mv: settings.voltage_offset_mv,
      } as Partial<Record<KnobId, number | null>>,
      firstGpu ? firstGpu.static : emptyStatic,
    );
    if (v) {
      toast.error(violationMessage(v));
      return;
    }
    try {
      if (selectedId != null) await updatePreset(selectedId, { notes, settings });
      else await createPreset({ name, notes, settings });
      mode = "list";
      toast.success("preset saved");
    } catch (e) {
      toast.error(String(e));
    }
  }

  function presetUsers(p: Preset): string[] {
    return Object.entries(status?.active_sources ?? {})
      .filter(([, s]) => s === `preset:${p.id}`)
      .map(([slot]) => slot);
  }

  function requestDelete(p: Preset) {
    pendingDelete = p;
    if (presetUsers(p).length > 0) {
      openModal("preset-delete-blocked");
      return;
    }
    openModal("confirm-delete");
  }

  async function confirmDelete() {
    const p = pendingDelete;
    pendingDelete = null;
    if (p?.id == null) return;
    try {
      await deletePreset(p.id);
      toast.success("deleted");
    } catch (e) {
      toast.error(String(e));
    }
  }

  async function doDuplicate(p: Preset) {
    try {
      const exp = await exportPreset(p.id!);
      await createPreset({ name: `${exp.name} (copy)`, notes: exp.notes, settings: exp.settings });
    } catch (e) {
      toast.error(String(e));
    }
  }
</script>

<div class="p-4">
  {#if mode === "list"}
    <div class="mb-3 flex gap-2">
      <Button variant="primary" icon={Plus} data-testid="new-preset-btn" onclick={startNew}>new preset</Button>
    </div>
    <PresetList {presets} onOpen={startEdit} onDelete={requestDelete} onDuplicate={doDuplicate} />
  {:else}
    <PresetEditor
      bind:name
      bind:notes
      {form}
      static={firstGpu ? firstGpu.static : emptyStatic}
      readonly={isBuiltin}
    />
    <div class="mt-4 flex gap-2">
      {#if !isBuiltin}
        <Button variant="primary" data-testid="save-btn" onclick={save}>save preset</Button>
      {:else}
        <p class="text-xs text-faint">built-in preset — read-only (re-created with shipped values on every daemon start)</p>
      {/if}
      <Button variant="ghost" onclick={() => (mode = "list")}>cancel</Button>
    </div>
  {/if}

  <ConfirmDialog
    id="confirm-delete"
    title="Delete preset"
    body={
      pendingDelete
        ? `Delete "${pendingDelete.name}"?` +
          (pendingDelete.is_builtin
            ? " This is a built-in preset — it will be re-created on the next daemon start."
            : " This cannot be undone.")
        : ""
    }
    confirmLabel="delete"
    onConfirm={confirmDelete}
  />

  <Modal id="preset-delete-blocked" title="Can't delete preset">
    {#if pendingDelete}
      <p class="text-danger">
        '{pendingDelete.name}' is applied to {presetUsers(pendingDelete).length} GPU
        {presetUsers(pendingDelete).length === 1 ? "" : "s"}. Switch them off it first.
      </p>
      <div class="mt-4 flex justify-end">
        <Button variant="secondary" onclick={closeModal}>Dismiss</Button>
      </div>
    {/if}
  </Modal>
</div>
