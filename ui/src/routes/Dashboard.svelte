<script lang="ts">
  import { getStatus, getTelemetry, listPresets } from "../lib/api";
  import GpuTile from "../lib/components/GpuTile.svelte";
  import LineChart from "../lib/components/charts/LineChart.svelte";
  import type { LineSeries } from "../lib/components/charts/LineChart.svelte";
  import EmptyState from "../lib/components/ui/EmptyState.svelte";
  import Card from "../lib/components/ui/Card.svelte";
  import { feedDown, seedFromRing } from "../lib/store/telemetry.svelte";
  import { sharedDomain } from "../lib/logic/chart";
  import { makeLatestOnly } from "../lib/logic/latest";
  import { Cpu } from "@lucide/svelte";
  import type { ChartReference } from "../lib/components/charts/LineChart.svelte";
  import type { GpuSample, GpuStatusEntry, Preset, Status } from "../lib/types";

  let {
    samples,
    rings,
  }: { samples: Record<string, GpuSample>; rings: Record<string, GpuSample[]> } = $props();

  let status = $state<Status | null>(null);
  let presets = $state<Preset[]>([]);

  // Race-safe loaders: the 5s poll can overlap a post-mutation refresh, and
  // without ordering a stale (older) response would overwrite the fresher one —
  // e.g. hiding a preset that was just created. makeLatestOnly drops stale
  // responses so only the most recent read ever lands.
  const applyStatus = makeLatestOnly<Status | null>((s) => (status = s));
  const applyPresets = makeLatestOnly<Preset[]>((p) => (presets = p));
  function loadStatus() {
    void applyStatus(getStatus);
  }
  function loadPresets() {
    void applyPresets(listPresets);
  }

  $effect(() => {
    const tick = () => {
      loadStatus();
      loadPresets();
    };
    tick();
    const t = setInterval(tick, 5000);
    return () => clearInterval(t);
  });

  $effect(() => {
    getTelemetry()
      .then((snap) => seedFromRing(snap.ring))
      .catch(() => {});
  });

  const gpus = $derived(status?.gpus ?? []);

  // One auto-scaled domain for every GPU's voltages chart, so the cards stay
  // visually comparable across GPUs (spec §2.3). `undefined` until any
  // voltage data has arrived → per-chart auto-scale fallback.
  const voltageDomain = $derived.by(() =>
    sharedDomain(
      gpus.flatMap((g) =>
        (rings[g.slot] ?? []).flatMap((s) => [
          s.voltage_gfx_mv,
          s.voltage_soc_mv,
          s.voltage_mem_mv,
        ]),
      ),
    ),
  );

  // Per-sensor palette (consistent across GPUs, tuned for the dark theme).
  const TEMP_COLORS = {
    edge: "var(--color-info)",
    hotspot: "var(--color-warn)",
    vram: "var(--color-success)",
  } as const;
  const POWER_COLOR = "var(--color-accent)";
  const FAN_COLOR = "var(--color-violet)";
  const RAM_COLOR = "var(--color-fuchsia)";
  const GFX_COLOR = "var(--color-accent)";
  const MEM_COLOR = "var(--color-violet)";
  const VOLTAGE_GFX_COLOR = "var(--color-accent)";
  const VOLTAGE_SOC_COLOR = "var(--color-info)";
  const VOLTAGE_MEM_COLOR = "var(--color-violet)";
  const TEMP_Y_DOMAIN: [number, number] = [20, 120];
  const POWER_Y_DOMAIN: [number, number] = [0, 350];
  const FAN_Y_DOMAIN: [number, number] = [0, 5000];
  const RAM_Y_DOMAIN: [number, number] = [0, 32];
  const CLOCK_Y_DOMAIN: [number, number] = [0, 3500];

  function tempSeriesFor(g: GpuStatusEntry): LineSeries[] {
    const ring = rings[g.slot] ?? [];
    return [
      {
        id: `${g.slot}-hotspot`,
        label: "hotspot",
        color: TEMP_COLORS.hotspot,
        valueMode: "range",
        avg: true,
        avgOrder: 3,
        points: ring.map((s) => ({ t: s.ts_ms, v: s.hotspot_c })),
      },
      {
        id: `${g.slot}-edge`,
        label: "edge",
        color: TEMP_COLORS.edge,
        valueMode: "range",
        avg: true,
        avgOrder: 1,
        points: ring.map((s) => ({ t: s.ts_ms, v: s.edge_c })),
      },
      {
        id: `${g.slot}-vram`,
        label: "vram",
        color: TEMP_COLORS.vram,
        valueMode: "range",
        avg: true,
        avgOrder: 2,
        points: ring.map((s) => ({ t: s.ts_ms, v: s.vram_c })),
      },
      {
        id: `${g.slot}-vramgb`,
        label: "vram",
        color: RAM_COLOR,
        decimals: 1,
        avg: true,
        avgOrder: 1,
        points: ring.map((s) => ({
          t: s.ts_ms,
          v: s.vram_used_mb != null ? s.vram_used_mb / 1024 : null,
        })),
      },
    ];
  }

  function tempRefsFor(g: GpuStatusEntry): ChartReference[] {
    const refs: ChartReference[] = [];
    if (g.static.slowdown_hotspot_c != null) {
      refs.push({ value: g.static.slowdown_hotspot_c, label: "hotspot max", color: TEMP_COLORS.hotspot });
    }
    if (g.static.slowdown_vram_c != null) {
      refs.push({ value: g.static.slowdown_vram_c, label: "vram max", color: TEMP_COLORS.vram, labelBelow: true });
    }
    return refs;
  }

  function powerSeriesFor(g: GpuStatusEntry): LineSeries[] {
    const ring = rings[g.slot] ?? [];
    return [
      {
        id: `${g.slot}-power`,
        label: "power",
        color: POWER_COLOR,
        avg: true,
        avgOrder: 1,
        points: ring.map((s) => ({ t: s.ts_ms, v: s.socket_power_w })),
      },
      {
        id: `${g.slot}-fan`,
        label: "fan",
        color: FAN_COLOR,
        avg: true,
        avgOrder: 1,
        points: ring.map((s) => ({ t: s.ts_ms, v: s.fan_rpm })),
      },
    ];
  }

  function clockSeriesFor(g: GpuStatusEntry): LineSeries[] {
    const ring = rings[g.slot] ?? [];
    return [
      {
        id: `${g.slot}-gfxutil`,
        label: "gfx util",
        color: GFX_COLOR,
        avg: true,
        avgOrder: 1,
        points: ring.map((s) => ({ t: s.ts_ms, v: s.gfx_activity_pct })),
      },
      {
        id: `${g.slot}-memutil`,
        label: "mem util",
        color: MEM_COLOR,
        avg: true,
        avgOrder: 2,
        points: ring.map((s) => ({ t: s.ts_ms, v: s.mem_activity_pct })),
      },
      {
        id: `${g.slot}-gfx`,
        label: "gfx",
        color: GFX_COLOR,
        points: ring.map((s) => ({ t: s.ts_ms, v: s.dpm_sclk_mhz })),
      },
      {
        id: `${g.slot}-mem`,
        label: "mem",
        color: MEM_COLOR,
        points: ring.map((s) => ({ t: s.ts_ms, v: s.dpm_mclk_mhz })),
      },
    ];
  }

  function voltageSeriesFor(g: GpuStatusEntry): LineSeries[] {
    const ring = rings[g.slot] ?? [];
    return [
      {
        id: `${g.slot}-vgfx`,
        label: "gfx",
        color: VOLTAGE_GFX_COLOR,
        points: ring.map((s) => ({ t: s.ts_ms, v: s.voltage_gfx_mv })),
      },
      {
        id: `${g.slot}-vsoc`,
        label: "soc",
        color: VOLTAGE_SOC_COLOR,
        points: ring.map((s) => ({ t: s.ts_ms, v: s.voltage_soc_mv })),
      },
      {
        id: `${g.slot}-vmem`,
        label: "mem",
        color: VOLTAGE_MEM_COLOR,
        points: ring.map((s) => ({ t: s.ts_ms, v: s.voltage_mem_mv })),
      },
    ];
  }
</script>

{#if gpus.length === 0}
  <div class="p-4">
    <EmptyState icon={Cpu} heading="Waiting for GPU status…" subtext="no GPUs reported yet" />
  </div>
{:else}
  <div class="grid gap-4 p-4 items-start" style="grid-template-columns: repeat(auto-fill, minmax(360px, 1fr))">
    {#each gpus as g (g.slot)}
      <div class="flex flex-col gap-4 min-w-0">
        <GpuTile
          sample={samples[g.slot] ?? null}
          ring={rings[g.slot] ?? []}
          staticInfo={g.static}
          stale={feedDown}
          {presets}
          busy={status?.busy ?? false}
          activeSource={status?.active_sources?.[g.slot] ?? null}
          activeSettings={status?.active_state?.settings?.[g.slot] ?? null}
          drifted={g.drifted === true}
          onPresetsChanged={loadPresets}
          onStatusChanged={loadStatus}
        />
        <Card>
          <LineChart
            series={tempSeriesFor(g)}
            references={tempRefsFor(g)}
            unit="°C"
            title={`Temperatures · ${g.slot}`}
            yDomain={TEMP_Y_DOMAIN}
            secondary={{ seriesIds: [`${g.slot}-vramgb`], unit: "GB", yDomain: RAM_Y_DOMAIN }}
            height={160}
          />
        </Card>
        <Card>
          <LineChart
            series={powerSeriesFor(g)}
            unit="W"
            legendTight
            title={`Power & Fan · ${g.slot}`}
            yDomain={POWER_Y_DOMAIN}
            secondary={{
              seriesIds: [`${g.slot}-fan`],
              unit: "RPM",
              yDomain: FAN_Y_DOMAIN,
              format: (v) => (v >= 1000 ? `${v / 1000}K` : String(v)),
            }}
            height={160}
          />
        </Card>
        <Card>
          <LineChart
            series={clockSeriesFor(g)}
            unit="MHz"
            legendTight
            title={`Clocks & Utilization · ${g.slot}`}
            yDomain={CLOCK_Y_DOMAIN}
            secondary={{
              seriesIds: [`${g.slot}-gfxutil`, `${g.slot}-memutil`],
              unit: "%",
              yDomain: [0, 100],
            }}
            height={160}
          />
        </Card>
        <Card>
          <LineChart
            series={voltageSeriesFor(g)}
            unit="mV"
            legendTight
            title={`Voltages · ${g.slot}`}
            yDomain={voltageDomain}
            height={160}
          />
        </Card>
      </div>
    {/each}
  </div>
{/if}
