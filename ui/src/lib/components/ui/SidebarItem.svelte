<script lang="ts">
  import type { Component } from "svelte";
  import type { HTMLAttributes } from "svelte/elements";
  import Icon from "./Icon.svelte";

  let {
    label,
    icon,
    href,
    active,
    collapsed,
    ...rest
  }: {
    label: string;
    icon: Component;
    href: string;
    active: boolean;
    collapsed: boolean;
  } & HTMLAttributes<HTMLAnchorElement> = $props();
</script>

<a
  {href}
  title={label}
  aria-label={label}
  aria-current={active ? "page" : undefined}
  class="flex items-center gap-3 rounded-control px-3 py-2 text-sm transition-colors duration-[var(--duration-fast)] ease-[var(--ease-out)] {collapsed ? "justify-center" : ""} {active ? "bg-surface-2 text-text" : "text-muted hover:bg-surface-2 hover:text-text"}"
  {...rest}
>
  <Icon {icon} class={active ? "text-accent" : ""} />
  {#if !collapsed}
    <span class="truncate">{label}</span>
  {/if}
</a>
