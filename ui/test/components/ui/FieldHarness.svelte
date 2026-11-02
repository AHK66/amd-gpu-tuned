<script lang="ts">
  import Field from "../../../src/lib/components/ui/Field.svelte";
  import Tooltip from "../../../src/lib/components/ui/Tooltip.svelte";

  let { withTooltip = false }: { withTooltip?: boolean } = $props();
</script>

{#if withTooltip}
  {#snippet labelTip()}
    <Tooltip text="tip text"><span>labeled</span></Tooltip>
  {/snippet}
  <Field labelSnippet={labelTip}>
    <input data-testid="field-input" />
  </Field>
{:else}
  <Field label="labeled">
    <input data-testid="field-input" />
  </Field>
{/if}
