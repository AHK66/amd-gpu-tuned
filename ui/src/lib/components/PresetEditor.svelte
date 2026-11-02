<script lang="ts">
  import type { SlotForm } from "../logic/form";
  import type { GpuStatic } from "../types";
  import Card from "./ui/Card.svelte";
  import Field from "./ui/Field.svelte";
  import TextInput from "./ui/TextInput.svelte";
  import Tooltip from "./ui/Tooltip.svelte";
  import { knobTooltip } from "../logic/knobs";

  let {
    name = $bindable(""),
    notes = $bindable(""),
    form,
    static: card,
    readonly = false,
  }: {
    name: string;
    notes: string;
    form: SlotForm;
    static: GpuStatic;
    readonly?: boolean;
  } = $props();
</script>

<div class="space-y-4 text-sm text-text">
  <div class="flex gap-2">
    <TextInput bind:value={name} placeholder="preset name" data-testid="preset-name" class="w-56" {readonly} />
    <TextInput bind:value={notes} placeholder="notes" {readonly} />
  </div>

  <Card data-testid="preset-card">
    <div class="grid grid-cols-2 gap-3 md:grid-cols-4">
      {#snippet powerCapLabel()}
        <Tooltip text={knobTooltip("power_cap_w", card)}>power cap W</Tooltip>
      {/snippet}
      <Field labelSnippet={powerCapLabel}>
        <TextInput type="text" inputmode="decimal" bind:value={form.power_cap_w} data-testid="power-cap" {readonly} />
      </Field>
      {#snippet sclkLabel()}
        <Tooltip text={knobTooltip("sclk_offset_mhz", card)}>sclk offset MHz</Tooltip>
      {/snippet}
      <Field labelSnippet={sclkLabel}>
        <TextInput type="text" inputmode="decimal" bind:value={form.sclk_offset_mhz} data-testid="sclk-offset" {readonly} />
      </Field>
      {#snippet maxMclkLabel()}
        <Tooltip text={knobTooltip("max_mclk_mhz", card)}>max mclk MHz</Tooltip>
      {/snippet}
      <Field labelSnippet={maxMclkLabel}>
        <TextInput type="text" inputmode="decimal" bind:value={form.max_mclk_mhz} data-testid="max-mclk" {readonly} />
      </Field>
      {#snippet voltageLabel()}
        <Tooltip text={knobTooltip("voltage_offset_mv", card)}>voltage offset mV (≤ 0)</Tooltip>
      {/snippet}
      <Field labelSnippet={voltageLabel}>
        <TextInput type="text" inputmode="decimal" bind:value={form.voltage_offset_mv} {readonly} />
      </Field>
    </div>
  </Card>
</div>
