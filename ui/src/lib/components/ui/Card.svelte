<script lang="ts">
  import type { Snippet } from "svelte";
  import type { HTMLAttributes } from "svelte/elements";

  let {
    title,
    actions,
    children,
    class: className = "",
    ...rest
  }: {
    title?: Snippet;
    actions?: Snippet;
    children: Snippet;
    class?: string;
  } & Omit<HTMLAttributes<HTMLDivElement>, "title"> = $props();
</script>

<div class="rounded-panel border border-hairline bg-surface p-4 text-sm text-text {className}" {...rest}>
  {#if title || actions}
    <div class="mb-3 flex items-center justify-between gap-3">
      {#if title}{@render title()}{/if}
      {#if actions}{@render actions()}{/if}
    </div>
  {/if}
  {@render children()}
</div>
