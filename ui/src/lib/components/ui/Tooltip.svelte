<script lang="ts">
  import type { Snippet } from "svelte";

  let { text, children, class: className = "" }: { text: string; children: Snippet; class?: string } = $props();
  let id = `tip-${Math.random().toString(36).slice(2, 10)}`;
</script>

<span class={`group relative inline-flex max-w-full ${className}`.trim()} aria-describedby={id}>
  {@render children()}
  <span
    id={id}
    role="tooltip"
    class="pointer-events-none absolute bottom-full left-0 z-10 mb-1 hidden w-max max-w-[260px] rounded-md border border-hairline bg-surface-2 px-2 py-1 text-left text-xs text-muted group-hover:block group-focus-within:block"
  >
    {text}
  </span>
</span>
