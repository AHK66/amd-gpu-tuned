<script lang="ts">
  import { getContext } from "svelte";
  import type { Snippet } from "svelte";
  import type { HTMLSelectAttributes } from "svelte/elements";

  let {
    value = $bindable(""),
    id,
    children,
    readonly = false,
    class: className = "",
    ...rest
  }: {
    value?: string;
    id?: string;
    children: Snippet;
    readonly?: boolean;
    class?: string;
  } & HTMLSelectAttributes = $props();

  const fieldId = getContext<string | undefined>("amd-gpu-tuned:fieldId");
  const selectId = $derived(id ?? fieldId);
</script>

<select
  bind:value
  id={selectId}
  disabled={readonly}
  class="h-9 rounded-control border border-hairline bg-bg px-2 text-sm text-text {className}"
  {...rest}
>
  {@render children()}
</select>
