<script lang="ts">
  import type { Component } from "svelte";
  import Icon from "./Icon.svelte";
  import Tooltip from "./Tooltip.svelte";

  let {
    icon,
    tone = "warn",
    text,
    testid,
    class: className = "",
  }: {
    icon: Component;
    tone?: "warn" | "info" | "danger";
    text: string;
    testid?: string;
    class?: string;
  } = $props();

  const TONES: Record<string, string> = {
    warn: "bg-warn/20 text-warn",
    info: "bg-info/20 text-info",
    danger: "bg-danger/20 text-danger",
  };
</script>

<Tooltip text={text} class={className}>
  <span
    data-testid={testid}
    role="img"
    aria-label={text}
    class="inline-flex h-[22px] w-[22px] shrink-0 items-center justify-center rounded {TONES[tone]}"
  >
    <Icon {icon} size={14} />
  </span>
</Tooltip>
