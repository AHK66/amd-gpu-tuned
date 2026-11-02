<script lang="ts">
  import type { Snippet } from "svelte";

  let { children }: { children: Snippet } = $props();
</script>

<th class="py-2 pr-3 text-left text-xs font-normal text-faint">
  {@render children()}
</th>
