<script lang="ts">
  import type { Snippet } from "svelte";
  import type { HTMLAttributes } from "svelte/elements";

  let {
    tone = "neutral",
    children,
    class: className = "",
    ...rest
  }: {
    tone?: "neutral" | "info" | "success" | "warn" | "danger";
    children: Snippet;
    class?: string;
  } & HTMLAttributes<HTMLSpanElement> = $props();

  const TONES: Record<string, string> = {
    neutral: "bg-surface-2 text-muted",
    info: "bg-info/20 text-info",
    success: "bg-success/20 text-success",
    warn: "bg-warn/20 text-warn",
    danger: "bg-danger/20 text-danger",
  };
</script>

<span class="rounded-pill px-2 py-0.5 text-xs {TONES[tone]} {className}" {...rest}>
  {@render children()}
</span>
