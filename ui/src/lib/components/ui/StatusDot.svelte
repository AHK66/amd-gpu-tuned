<script lang="ts">
  import type { HTMLAttributes } from "svelte/elements";

  let {
    color = "neutral",
    pulse = false,
    class: className = "",
    ...rest
  }: {
    color?: "neutral" | "info" | "success" | "warn" | "danger";
    pulse?: boolean;
    class?: string;
  } & HTMLAttributes<HTMLSpanElement> = $props();

  const COLORS: Record<string, string> = {
    neutral: "bg-faint",
    info: "bg-info",
    success: "bg-success",
    warn: "bg-warn",
    danger: "bg-danger",
  };
</script>

<span
  aria-hidden="true"
  class="inline-block h-2 w-2 rounded-full {COLORS[color]} {className}"
  {...rest}
></span>
