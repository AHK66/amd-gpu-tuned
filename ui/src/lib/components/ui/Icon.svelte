<script lang="ts">
  import type { Component, Snippet } from "svelte";

  let {
    icon,
    size = 18,
    class: className = "",
  }: {
    icon: Component;
    size?: number;
    class?: string;
  } = $props();

  // lucide-svelte ships Svelte 4-style component typings; @render needs a Snippet
  const render = $derived(
    icon as unknown as Snippet<[{ size: number; class: string }]>,
  );
</script>

{@render render({ size, class: className })}
