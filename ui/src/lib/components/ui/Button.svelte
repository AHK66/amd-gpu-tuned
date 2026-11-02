<script lang="ts">
  import type { Component, Snippet } from "svelte";
  import type { HTMLButtonAttributes } from "svelte/elements";
  import Icon from "./Icon.svelte";

  let {
    variant = "secondary",
    size = "md",
    pill = false,
    icon,
    children,
    class: className = "",
    ...rest
  }: {
    variant?: "primary" | "secondary" | "ghost" | "danger";
    size?: "sm" | "md";
    pill?: boolean;
    icon?: Component;
    children: Snippet;
    class?: string;
  } & HTMLButtonAttributes = $props();

  const VARIANTS: Record<string, string> = {
    primary: "bg-text text-bg hover:opacity-90",
    secondary: "bg-surface-2 text-text hover:opacity-80",
    ghost: "bg-transparent text-muted hover:bg-surface-2 hover:text-text",
    danger: "bg-danger/15 text-danger hover:bg-danger/25",
  };
  const SIZES: Record<string, string> = {
    sm: "h-8 px-3 text-xs",
    md: "h-9 px-4 text-sm",
  };
  const shape = $derived(pill ? "rounded-pill" : "rounded-control");
  const cls = $derived(`${VARIANTS[variant]} ${SIZES[size]} ${shape} inline-flex items-center gap-2 font-medium transition-opacity duration-[var(--duration-fast)] ease-[var(--ease-out)] ${className}`);
</script>

<button class={cls} {...rest}>
  {#if icon}
    <Icon {icon} size={16} />
  {/if}
  {@render children()}
</button>
