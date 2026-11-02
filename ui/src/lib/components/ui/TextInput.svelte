<script lang="ts">
  import { getContext } from "svelte";
  import type { HTMLInputAttributes } from "svelte/elements";

  let {
    value = $bindable(""),
    id,
    class: className = "",
    ...rest
  }: {
    value?: string;
    id?: string;
    class?: string;
  } & HTMLInputAttributes = $props();

  const fieldId = getContext<string | undefined>("amd-gpu-tuned:fieldId");
  const inputId = $derived(id ?? fieldId);
</script>

<input
  type="text"
  bind:value
  id={inputId}
  class="h-9 rounded-control border border-hairline bg-bg px-3 text-sm text-text {className}"
  {...rest}
/>
