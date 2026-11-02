<script lang="ts">
  import { setContext } from "svelte";
  import type { Snippet } from "svelte";

  let {
    label,
    labelSnippet,
    hint,
    children,
  }: {
    label?: string;
    labelSnippet?: Snippet;
    hint?: string;
    children: Snippet;
  } = $props();

  const id = `field-${Math.random().toString(36).slice(2, 10)}`;
  setContext("amd-gpu-tuned:fieldId", id);
</script>

<div class="flex flex-col gap-1">
  <label for={id} class="text-muted">
    {#if labelSnippet}
      {@render labelSnippet()}
    {:else}
      {label}
    {/if}
  </label>
  {@render children()}
  {#if hint}
    <span class="text-xs text-faint">{hint}</span>
  {/if}
</div>
