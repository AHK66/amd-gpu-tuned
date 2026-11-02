<script lang="ts">
  import type { Component, Snippet } from "svelte";
  import Icon from "./Icon.svelte";

  let {
    icon,
    heading,
    subtext,
    cta,
  }: {
    icon: Component;
    heading: string;
    subtext?: string;
    cta?: Snippet;
  } = $props();
</script>

<div class="flex flex-col items-center justify-center gap-2 py-16 text-center">
  <Icon {icon} size={28} class="text-faint" />
  <p class="text-text">{heading}</p>
  {#if subtext}
    <p class="text-faint">{subtext}</p>
  {/if}
  {#if cta}
    <div class="mt-2">{@render cta()}</div>
  {/if}
</div>
