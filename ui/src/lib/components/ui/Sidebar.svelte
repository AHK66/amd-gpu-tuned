<script lang="ts">
  import { onMount } from "svelte";
  import { route } from "../../router.svelte";
  import { feedDown } from "../../store/telemetry.svelte";
  import { getStatus, lockNow } from "../../api";
  import type { Status } from "../../types";
  import { authState } from "../../store/auth.svelte";
  import { openModal } from "../../store/modal.svelte";
  import { toast } from "../../store/toast.svelte";
  import SidebarItem from "./SidebarItem.svelte";
  import StatusDot from "./StatusDot.svelte";
  import ConfirmDialog from "./ConfirmDialog.svelte";
  import {
    LayoutDashboard,
    Sliders,
    Radar,
    Wrench,
    ShieldAlert,
    ChevronsLeft,
    ChevronsRight,
    Lock,
    LockOpen,
  } from "@lucide/svelte";

  const KEY = "amd-gpu-tuned:sidebar";
  let expanded = $state(true);
  let status = $state<Status | null>(null);

  onMount(() => {
    const saved = localStorage.getItem(KEY);
    if (saved != null) expanded = saved === "1";
    const tick = () => {
      getStatus()
        .then((s) => {
          status = s;
          authState.enabled = s.auth_enabled;
        })
        .catch(() => {});
    };
    tick();
    const t = setInterval(tick, 5000);
    return () => clearInterval(t);
  });

  $effect(() => {
    localStorage.setItem(KEY, expanded ? "1" : "0");
  });

  const items: { r: string; label: string; icon: typeof LayoutDashboard }[] = [
    { r: "", label: "Dashboard", icon: LayoutDashboard },
    { r: "presets", label: "Presets", icon: Sliders },
    { r: "drift", label: "Drift", icon: Radar },
    { r: "setup", label: "Setup", icon: Wrench },
  ];
</script>

<aside
  class="flex h-screen shrink-0 flex-col border-r border-hairline bg-surface transition-[width] duration-[var(--duration-sidebar)] ease-[var(--ease-out)]"
  style="width: {expanded ? '260px' : '52px'}"
>
  <div class="flex items-center justify-between gap-2 px-3 py-3">
    {#if expanded}
      <span class="text-sm font-semibold text-text">amd-gpu-tuned</span>
    {/if}
    <button
      data-testid="sidebar-toggle"
      class="text-faint hover:text-text"
      title={expanded ? "collapse" : "expand"}
      aria-label={expanded ? "collapse sidebar" : "expand sidebar"}
      onclick={() => (expanded = !expanded)}
    >
      {#if expanded}<ChevronsLeft size={18} />{:else}<ChevronsRight size={18} />{/if}
    </button>
  </div>

  <nav class="flex flex-1 flex-col gap-1 overflow-hidden px-2">
    {#each items as it (it.r)}
      <SidebarItem
        label={it.label}
        icon={it.icon}
        href="#/{it.r}"
        active={route() === it.r}
        collapsed={!expanded}
        data-testid="nav-{it.r === '' ? 'dashboard' : it.r}"
      />
    {/each}
    {#if status && status.privileged === false}
      {#if expanded}
        <div class="mt-1 flex items-start gap-2 rounded-md border border-warn/40 bg-warn/10 px-2 py-1.5 text-warn" data-testid="no-sudo-banner">
          <ShieldAlert size={14} class="mt-0.5 shrink-0" />
          <span class="text-[11px] leading-tight">No sudo — GPU writes blocked. Restart with <code class="font-mono">sudo</code>.</span>
        </div>
      {:else}
        <div class="mt-1 flex justify-center" data-testid="no-sudo-banner">
          <ShieldAlert size={16} class="text-warn" />
        </div>
      {/if}
    {/if}
  </nav>

  <div class="border-t border-hairline px-3 py-3 text-xs">
    {#if expanded}
      <div class="flex flex-col gap-1 text-faint">
        <div class="flex items-center gap-2">
          <StatusDot color={feedDown ? "danger" : "success"} data-testid="feed-lamp" />
          <span>overdrive: {status ? (status.active_state ? "ON" : "stock") : "…"}</span>
        </div>
        <span class="text-faint">amd-gpu-tuned v{status?.version ?? "…"}</span>
        {#if authState.enabled}
          {#if authState.unlocked}
            <button
              data-testid="lock-indicator"
              class="flex items-center gap-1 text-faint hover:text-text"
              title="lock writes now"
              onclick={() => openModal("confirm-lock")}
            >
              <LockOpen size={12} /> unlocked
            </button>
          {:else}
            <span data-testid="lock-indicator" class="flex items-center gap-1">
              <Lock size={12} /> locked
            </span>
          {/if}
        {/if}
      </div>
    {:else}
      <div class="flex flex-col items-center gap-2">
        <StatusDot color={feedDown ? "danger" : "success"} data-testid="feed-lamp" />
      </div>
    {/if}
  </div>

  <ConfirmDialog
    id="confirm-lock"
    title="Lock writes?"
    body="The unlock token is dropped; the next write asks for the password again."
    confirmLabel="lock now"
    onConfirm={async () => {
      try {
        await lockNow();
      } catch (e) {
        toast.error(String(e));
      }
    }}
  />
</aside>
