<script lang="ts">
  import { onMount } from "svelte";
  import { Radar } from "@lucide/svelte";
  import { clearDrift, getDrift, listPresets } from "../lib/api";
  import Badge from "../lib/components/ui/Badge.svelte";
  import Button from "../lib/components/ui/Button.svelte";
  import ConfirmDialog from "../lib/components/ui/ConfirmDialog.svelte";
  import Card from "../lib/components/ui/Card.svelte";
  import EmptyState from "../lib/components/ui/EmptyState.svelte";
  import Table from "../lib/components/ui/Table.svelte";
  import Th from "../lib/components/ui/Th.svelte";
  import { openModal } from "../lib/store/modal.svelte";
  import { toast } from "../lib/store/toast.svelte";
  import type { DriftEntry, DriftKnobs, DriftStatus, Preset } from "../lib/types";

  let data = $state<DriftStatus | null>(null);
  let presets = $state<Preset[]>([]);

  const OUTCOME_TONE: Record<DriftEntry["outcome"], "neutral" | "info" | "success" | "warn" | "danger"> = {
    reapplied: "success",
    reapply_failed: "danger",
    cleared: "neutral",
    logged_only: "neutral",
    adopted: "warn",
    pending: "info",
  };

  function load() {
    getDrift().then((d) => (data = d)).catch(() => {});
    listPresets().then((p) => (presets = p)).catch(() => {});
  }

  async function doClear() {
    try {
      const r = await clearDrift();
      toast.success(`drift history cleared (${r.cleared} entries)`);
      load();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : "clear failed");
    }
  }

  onMount(() => {
    load();
    const t = setInterval(load, 5000);
    return () => clearInterval(t);
  });

  function presetName(source: string): string {
    const m = source.match(/^preset:(\d+)$/);
    if (m) return presets.find((p) => p.id === Number(m[1]))?.name ?? source;
    return source;
  }

  function knobs(v: DriftKnobs | null): string {
    if (!v) return "—";
    const parts = [
      v.power_cap_w != null ? `${Math.round(v.power_cap_w)} W` : null,
      v.sclk_offset_mhz != null ? `sclk ${v.sclk_offset_mhz >= 0 ? "+" : ""}${v.sclk_offset_mhz}` : null,
      v.max_mclk_mhz != null ? `mclk ${v.max_mclk_mhz}` : null,
      v.voltage_offset_mv != null ? `${v.voltage_offset_mv} mV` : null,
    ].filter(Boolean);
    return parts.length > 0 ? parts.join(" · ") : "—";
  }

  const pendingSlots = $derived(Object.keys(data?.pending ?? {}));
</script>

<div class="p-4 text-sm text-text">
  <div class="mb-3 flex flex-wrap items-center gap-2">
    {#if data}
      <Badge tone={data.reapply ? "success" : "neutral"} data-testid="drift-mode">
        {data.reapply ? `auto-restore on (${data.delay_s} s)` : "auto-restore off"}
      </Badge>
      {#each pendingSlots as slot (slot)}
        <Badge tone="info" data-testid="drift-pending-badge" class="font-mono">
          {slot} · restoring
        </Badge>
      {/each}
      <Button
        variant="ghost"
        data-testid="drift-clear"
        disabled={data.entries.length === 0}
        onclick={() => openModal("drift-clear-confirm")}
      >
        clear history
      </Button>
    {/if}
  </div>

  {#if data && data.entries.length === 0}
    <EmptyState icon={Radar} heading="No drift events recorded." />
  {:else if data}
    <Card>
      <Table>
        {#snippet header()}
          <Th>Time</Th>
          <Th>Card</Th>
          <Th>Detected</Th>
          <Th>Intended</Th>
          <Th>Previous source</Th>
          <Th>Outcome</Th>
        {/snippet}
        {#each data.entries as e (e.id)}
          <tr class="border-t border-hairline">
            <td class="py-2 pr-3 whitespace-nowrap text-faint">
              {new Date(e.ts).toLocaleString()}
            </td>
            <td class="py-2 pr-3 font-mono text-xs">{e.slot}</td>
            <td class="py-2 pr-3 text-xs">{knobs(e.detected)}</td>
            <td class="py-2 pr-3 text-xs">{knobs(e.intended)}</td>
            <td class="py-2 pr-3 text-xs">{presetName(e.previous_source)}</td>
            <td class="py-2">
              <Badge tone={OUTCOME_TONE[e.outcome]} data-testid="drift-outcome-{e.id}">
                {e.outcome}
              </Badge>
            </td>
          </tr>
        {/each}
      </Table>
    </Card>
  {:else}
    <p class="text-faint">loading drift ledger…</p>
  {/if}
</div>

<ConfirmDialog
  id="drift-clear-confirm"
  title="Clear drift history"
  body={`Clear the drift history (${data?.entries.length ?? 0} entries)? This cannot be undone.`}
  confirmLabel="clear"
  onConfirm={doClear}
/>
