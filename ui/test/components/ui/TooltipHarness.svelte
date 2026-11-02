<script lang="ts">
  import Tooltip from "../../../src/lib/components/ui/Tooltip.svelte";

  let { text = "tip text" }: { text?: string } = $props();
</script>

<Tooltip {text}>
  <span data-testid="tip-trigger">trigger</span>
</Tooltip>
