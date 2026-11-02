<script lang="ts">
  import type { Snippet } from "svelte";
  import {
    downsampleLTTB,
    nearestByTime,
    niceTicks,
    pillWidth,
    rightMarginFor,
    scaleLinear,
    seriesMean,
    timeTicks,
  } from "../../logic/chart";

  export interface LinePoint {
    t: number;
    v: number | null;
  }
  export interface LineSeries {
    id: string;
    label: string;
    color: string;
    points: LinePoint[];
    decimals?: number;
    showValue?: boolean;
    valueMode?: "current" | "range";
    /** Render a window-average pill in the left margin. */
    avg?: boolean;
    /** Pill draw order: ascending; higher is drawn later (on top when overlapping). */
    avgOrder?: number;
  }
  export interface ChartReference {
    value: number;
    color?: string;
    label?: string;
    dash?: string;
    labelBelow?: boolean;
  }
  export interface SecondaryAxis {
    seriesIds: string[];
    unit: string;
    yDomain?: [number, number];
    format?: (v: number) => string;
  }

  let {
    series,
    unit = "",
    title,
    yDomain,
    xDomain,
    height = 160,
    overlay,
    references = [],
    secondary,
    legendTight = false,
  }: {
    series: LineSeries[];
    unit?: string;
    title: string;
    yDomain?: [number, number];
    xDomain?: [number, number];
    height?: number;
    overlay?: Snippet;
    references?: ChartReference[];
    secondary?: SecondaryAxis;
    legendTight?: boolean;
  } = $props();

  // Left/right margins are sized to the widest axis labels ("3500 MHz", "5K RPM")
  // plus a small buffer, so every chart's core plot is the same size and the
  // charts stay vertically synchronized in a column.
  const MARGIN = { top: 8, bottom: 22, left: 56 } as const;
  const BASE_RIGHT = 46;

  let containerEl = $state<HTMLElement | null>(null);
  let width = $state(0);

  $effect(() => {
    if (!containerEl) return;
    const ro = new ResizeObserver((entries) => {
      width = entries[0]?.contentRect.width ?? 0;
    });
    ro.observe(containerEl);
    return () => ro.disconnect();
  });

  const hidden = $state<Record<string, boolean>>({});

  const visibleSeries = $derived(series.filter((s) => !hidden[s.id]));

  const isSecondary = (s: LineSeries) =>
    secondary?.seriesIds.includes(s.id) ?? false;

  const primarySeries = $derived(visibleSeries.filter((s) => !isSecondary(s)));
  const secondarySeries = $derived(visibleSeries.filter((s) => isSecondary(s)));

  const allPoints = $derived(
    visibleSeries.flatMap((s) => s.points.filter((p) => p.v !== null)),
  );
  const primaryPoints = $derived(
    primarySeries.flatMap((s) => s.points.filter((p) => p.v !== null)),
  );
  const secondaryPoints = $derived(
    secondarySeries.flatMap((s) => s.points.filter((p) => p.v !== null)),
  );

  const hasData = $derived(allPoints.length > 0);
  const hasSecondaryData = $derived(secondaryPoints.length > 0);

  const xDom = $derived.by(() => {
    if (xDomain) return xDomain;
    if (allPoints.length === 0) return [0, 1] as [number, number];
    let min = Infinity;
    let max = -Infinity;
    for (const p of allPoints) {
      if (p.t < min) min = p.t;
      if (p.t > max) max = p.t;
    }
    if (min === max) max = min + 1;
    return [min, max] as [number, number];
  });

  const yDom = $derived.by(() => {
    if (yDomain) return yDomain;
    if (primaryPoints.length === 0) return [0, 1] as [number, number];
    let min = Infinity;
    let max = -Infinity;
    for (const p of primaryPoints) {
      const v = p.v!;
      if (v < min) min = v;
      if (v > max) max = v;
    }
    return niceTicks(min, max).domain;
  });

  const yDom2 = $derived.by((): [number, number] => {
    if (!hasSecondaryData) return [0, 1] as [number, number];
    if (secondary?.yDomain) {
      const [lo, hi] = secondary.yDomain;
      let max = hi;
      for (const p of secondaryPoints) if (p.v! > max) max = p.v!;
      return [lo, Math.max(hi, max)] as [number, number];
    }
    let min = Infinity;
    let max = -Infinity;
    for (const p of secondaryPoints) {
      const v = p.v!;
      if (v < min) min = v;
      if (v > max) max = v;
    }
    return niceTicks(min, max).domain;
  });

  // Clamp ticks to the domain so a nice-rounded step (e.g. [0,32] → 35) can't
  // push the unit-bearing top tick off-plot.
  const yTicks = $derived(
    niceTicks(yDom[0], yDom[1]).ticks.filter((t) => t >= yDom[0] && t <= yDom[1]),
  );
  const yTicks2 = $derived(
    hasSecondaryData
      ? niceTicks(yDom2[0], yDom2[1]).ticks.filter(
          (t) => t >= yDom2[0] && t <= yDom2[1],
        )
      : [],
  );
  const secondaryColor = $derived(
    secondarySeries[0]?.color ?? "var(--color-faint, #9ca3af)",
  );
  const xTicks = $derived(timeTicks(xDom[0], xDom[1]));

  const plotH = $derived(height - MARGIN.top - MARGIN.bottom);

  const scaleY = $derived(scaleLinear(yDom, [MARGIN.top + plotH, MARGIN.top]));
  const scaleY2 = $derived(scaleLinear(yDom2, [MARGIN.top + plotH, MARGIN.top]));

  interface AvgMarker {
    id: string;
    color: string;
    y: number;
    value: number;
    width: number;
    side: "left" | "right";
    order: number;
    label: string;
    unit: string;
  }

  // Window-average pills for series with `avg: true`, one per series on the
  // side of its own axis (left = primary scale, right = secondary scale).
  // Drawn in `avgOrder` ascending (higher = later = on top when overlapping).
  // A pill whose mean falls outside the plot (e.g. a fixed yDomain) is omitted —
  // same boundary semantics as the reference-line guard.
  const avgMarkers = $derived<AvgMarker[]>(
    visibleSeries
      .filter((s) => s.avg === true)
      .map((s) => {
        const mean = seriesMean(s.points);
        if (mean === null) return null;
        const side: "left" | "right" = isSecondary(s) ? "right" : "left";
        const y = (side === "right" ? scaleY2 : scaleY)(mean);
        if (y < MARGIN.top - 0.5 || y > MARGIN.top + plotH + 0.5) return null;
        const value = Math.round(mean);
        return {
          id: s.id,
          color: s.color,
          y,
          value,
          width: pillWidth(value),
          side,
          order: s.avgOrder ?? 0,
          label: s.label,
          unit: isSecondary(s) ? (secondary?.unit ?? "") : unit,
        };
      })
      .filter((m): m is AvgMarker => m !== null)
      .sort((a, b) => a.order - b.order),
  );

  const widestRightPill = $derived(
    avgMarkers.reduce((mx, m) => (m.side === "right" ? Math.max(mx, m.width) : mx), 0),
  );
  // Deterministic estimate of the widest secondary tick-label string (the
  // topmost tick carries the unit). 6px/char at ~10px font; tune visually.
  const estRightTickWidth = $derived(
    secondary && yTicks2.length > 0
      ? Math.max(
          ...yTicks2.map((t, i) => {
            const label = secondary.format ? secondary.format(t) : String(t);
            return (i === yTicks2.length - 1 ? `${label} ${secondary.unit}` : label).length;
          }),
        ) * 6
      : 0,
  );
  const rightMargin = $derived(
    rightMarginFor({ widestPill: widestRightPill, tickLabelWidth: estRightTickWidth, base: BASE_RIGHT }),
  );

  const plotW = $derived(Math.max(0, width - MARGIN.left - rightMargin));
  const scaleX = $derived(scaleLinear(xDom, [MARGIN.left, MARGIN.left + plotW]));

  interface SegmentedPath {
    d: string;
    color: string;
    muted: boolean;
  }

  const linePaths = $derived<SegmentedPath[]>(
    visibleSeries.map((s) => {
      const yScale = isSecondary(s) ? scaleY2 : scaleY;
      const segments: { t: number; v: number }[][] = [];
      let segStart = 0;
      for (let i = 0; i <= s.points.length; i++) {
        if (i === s.points.length || s.points[i].v === null) {
          if (i > segStart) {
            segments.push(s.points.slice(segStart, i) as { t: number; v: number }[]);
          }
          segStart = i + 1;
        }
      }
      let d = "";
      for (const seg of segments) {
        const pts = downsampleLTTB(seg, Math.max(2, 2 * plotW));
        let inSegment = false;
        for (const p of pts) {
          const x = scaleX(p.t);
          const y = yScale(p.v);
          d += inSegment ? `L${x.toFixed(1)},${y.toFixed(1)}` : `M${x.toFixed(1)},${y.toFixed(1)}`;
          inSegment = true;
        }
      }
      return { d, color: s.color, muted: isSecondary(s) };
    }),
  );

  const areaPaths = $derived(
    visibleSeries.map((s) => {
      if (isSecondary(s)) return null;
      const valid = s.points.filter((p) => p.v !== null);
      if (valid.length < 2) return null;
      const bottom = (MARGIN.top + plotH).toFixed(1);
      let d = "";
      let inSegment = false;
      let lastX = 0;
      for (const p of s.points) {
        if (p.v === null) {
          if (inSegment) {
            d += `L${lastX.toFixed(1)},${bottom}Z`;
            inSegment = false;
          }
          continue;
        }
        const x = scaleX(p.t);
        const y = scaleY(p.v);
        if (!inSegment) {
          d += `M${x.toFixed(1)},${bottom}L${x.toFixed(1)},${y.toFixed(1)}`;
          inSegment = true;
        } else {
          d += `L${x.toFixed(1)},${y.toFixed(1)}`;
        }
        lastX = x;
      }
      if (inSegment) {
        d += `L${lastX.toFixed(1)},${bottom}Z`;
      }
      return d || null;
    }),
  );

  let crosshairX = $state<number | null>(null);
  let tooltipData = $state<
    { x: number; entries: { label: string; value: number; unit: string }[] } | null
  >(null);
  // Tooltip shown when hovering an average pill: explains the value is a
  // window average. Opens into the plot (left pills rightward, right pills
  // leftward) so it never clips at the margin edge.
  let avgTooltip = $state<{ left: number; y: number; text: string; flip: boolean } | null>(
    null,
  );

  function onPointerMove(e: PointerEvent): void {
    if (!containerEl || !hasData) return;
    const rect = containerEl.getBoundingClientRect();
    const mouseX = e.clientX - rect.left;
    if (mouseX < MARGIN.left || mouseX > MARGIN.left + plotW) {
      crosshairX = null;
      tooltipData = null;
      return;
    }
    const tAtMouse = xDom[0] + ((mouseX - MARGIN.left) / plotW) * (xDom[1] - xDom[0]);

    const union = allPoints;
    const idx = nearestByTime(union, tAtMouse);
    if (idx === -1) {
      crosshairX = null;
      tooltipData = null;
      return;
    }
    const nearestT = union[idx].t;
    crosshairX = scaleX(nearestT);

    const entries: { label: string; value: number; unit: string }[] = [];
    for (const s of visibleSeries) {
      const sIdx = nearestByTime(s.points, nearestT);
      if (sIdx !== -1 && Math.abs(s.points[sIdx].t - nearestT) <= 5000) {
        const v = s.points[sIdx].v;
        if (v !== null) {
          const sUnit = isSecondary(s) ? (secondary?.unit ?? "") : unit;
          entries.push({ label: s.label, value: v, unit: sUnit });
        }
      }
    }
    tooltipData = { x: crosshairX, entries };
  }

  function onPointerLeave(): void {
    crosshairX = null;
    tooltipData = null;
  }

  function onAvgEnter(m: AvgMarker): void {
    avgTooltip = {
      left: m.side === "right" ? width - 2 - m.width : 2 + m.width,
      y: m.y,
      text: `${m.label} · window average: ${m.value}${m.unit ? ` ${m.unit}` : ""}`,
      flip: m.side === "right",
    };
  }

  function onAvgLeave(): void {
    avgTooltip = null;
  }

  function toggleSeries(id: string): void {
    hidden[id] = !hidden[id];
  }

  function fmtTime(t: number): string {
    const d = new Date(t);
    return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
  }

  // Legend chip value, tagged with the series' axis unit, e.g. "248 W" or
  // "3420 RPM". "current" mode shows the most recent sample — "—" (not a
  // stale reading) when the latest sample has no value. "range" mode shows
  // min–max over the visible window with an en dash ("N–N" when min == max,
  // "—" when the series has no data); `decimals` apply to both numbers.
  // Suppressed when showValue is false.
  function legendValue(s: LineSeries): string {
    if (s.showValue === false) return "";
    const u = isSecondary(s) ? (secondary?.unit ?? "") : unit;
    const suffix = u !== "" ? ` ${u}` : "";
    const d = s.decimals ?? 0;
    if (s.valueMode === "range") {
      const vs = s.points.map((p) => p.v).filter((v): v is number => v !== null);
      if (vs.length === 0) return `—${suffix}`;
      const lo = Math.min(...vs);
      const hi = Math.max(...vs);
      return `${lo.toFixed(d)}–${hi.toFixed(d)}${suffix}`;
    }
    const v = s.points[s.points.length - 1]?.v ?? null;
    return v === null ? `—${suffix}` : `${v.toFixed(d)}${suffix}`;
  }

  const tooltipFlip = $derived(
    tooltipData !== null && containerEl !== null &&
    tooltipData.x + 140 > (containerEl?.clientWidth ?? 0),
  );

  const legendClass = "mb-1 grid gap-2";
  const legendStyle = $derived(
    `padding-left: ${MARGIN.left}px; padding-right: ${rightMargin}px; grid-template-columns: repeat(2, 1fr); justify-items: start;`,
  );
</script>

<div bind:this={containerEl} class="relative min-w-[280px]" style="height: {height}px">
  {#if width > 0}
    <svg
      width={width}
      {height}
      viewBox="0 0 {width} {height}"
      role="img"
      aria-label={title}
      onpointermove={onPointerMove}
      onpointerleave={onPointerLeave}
    >
      {#if hasData}
        <!-- Grid -->
        {#each yTicks as tick (tick)}
          <line
            x1={MARGIN.left}
            x2={MARGIN.left + plotW}
            y1={scaleY(tick)}
            y2={scaleY(tick)}
            stroke="var(--color-hairline, #e5e7eb)"
            stroke-width="1"
          />
        {/each}

        <!-- Area fills -->
        {#each areaPaths as areaD, i (visibleSeries[i].id)}
          {#if areaD}
            <path d={areaD} fill={visibleSeries[i].color} fill-opacity="0.1" stroke="none" />
          {/if}
        {/each}

        <!-- Lines -->
        {#each linePaths as lp, i (visibleSeries[i].id)}
          <path
            d={lp.d}
            data-series={visibleSeries[i].id}
            fill="none"
            stroke={lp.color}
            stroke-width={lp.muted ? "1.25" : "1.75"}
            stroke-dasharray={lp.muted ? "4 3" : undefined}
            opacity={lp.muted ? "0.7" : undefined}
            stroke-linejoin="round"
          />
        {/each}

        <!-- Reference lines (maxima) -->
        {#each references as ref, ri (ref.value + ":" + (ref.label ?? ri))}
          {@const ry = scaleY(ref.value)}
          {#if ry >= MARGIN.top - 0.5 && ry <= MARGIN.top + plotH + 0.5}
            <line
              x1={MARGIN.left}
              x2={MARGIN.left + plotW}
              y1={ry}
              y2={ry}
              data-reference={ref.label ?? String(ref.value)}
              stroke={ref.color ?? "var(--color-faint, #afafaf)"}
              stroke-width="1"
              stroke-dasharray={ref.dash ?? "4 4"}
              aria-hidden="true"
            />
            {#if ref.label}
              <text
                x={MARGIN.left + plotW}
                y={ref.labelBelow ? ry + 11 : ry - 3}
                text-anchor="end"
                font-size="9"
                fill={ref.color ?? "var(--color-faint, #afafaf)"}
                opacity="0.75"
              >{ref.label}</text>
            {/if}
          {/if}
        {/each}

        <!-- Overlay -->
        {#if overlay}
          {@render overlay()}
        {/if}

        <!-- Crosshair -->
        {#if crosshairX !== null}
          <line
            x1={crosshairX}
            x2={crosshairX}
            y1={MARGIN.top}
            y2={MARGIN.top + plotH}
            stroke="var(--color-hairline, #e5e7eb)"
            stroke-width="1"
            aria-hidden="true"
          />
        {/if}

        <!-- Y axis labels -->
        {#each yTicks as tick, i (tick)}
          <text
            x={MARGIN.left - 6}
            y={scaleY(tick) + 4}
            text-anchor="end"
            font-size="10"
            fill="var(--color-faint, #9ca3af)"
          >{i === yTicks.length - 1 ? `${tick} ${unit}` : tick}</text>
        {/each}

        <!-- Average pills (left and right margins, per each series' own axis) -->
        {#each avgMarkers as m (m.id)}
          {@const rx = m.side === "right" ? width - 2 - m.width : 2}
          <g
            data-avg-marker={m.id}
            data-avg-side={m.side}
            aria-hidden="true"
            onpointerenter={() => onAvgEnter(m)}
            onpointerleave={onAvgLeave}
            style="cursor: help"
          >
            <rect
              x={rx}
              y={m.y - 7}
              width={m.width}
              height="14"
              rx="7"
              fill={m.color}
              opacity="0.9"
            />
            <text
              x={rx + m.width / 2}
              y={m.y + 3}
              text-anchor="middle"
              font-size="9"
              font-weight="600"
              fill="#18181b"
              style="font-variant-numeric: tabular-nums"
            >{m.value}</text>
          </g>
        {/each}

        <!-- Secondary (right) axis labels -->
        {#if secondary && hasSecondaryData}
          {#each yTicks2 as tick, i (tick)}
            {@const label = secondary.format ? secondary.format(tick) : String(tick)}
            <text
              x={MARGIN.left + plotW + 6}
              y={scaleY2(tick) + 4}
              text-anchor="start"
              font-size="10"
              fill={secondaryColor}
              opacity="0.7"
            >{i === yTicks2.length - 1 ? `${label} ${secondary.unit}` : label}</text>
          {/each}
        {/if}

        <!-- X axis labels -->
        {#each xTicks as t (t)}
          <text
            x={scaleX(t)}
            y={height - 4}
            text-anchor="middle"
            font-size="10"
            fill="var(--color-faint, #9ca3af)"
          >{fmtTime(t)}</text>
        {/each}
      {:else}
        <text
          x={width / 2}
          y={height / 2}
          text-anchor="middle"
          font-size="13"
          fill="var(--color-faint, #9ca3af)"
        >no data yet</text>
      {/if}
    </svg>

    <!-- Tooltip -->
    {#if tooltipData && tooltipData.entries.length > 0}
      <div
        data-testid="chart-tooltip"
        class="absolute pointer-events-none rounded border border-hairline bg-surface px-2 py-1 text-xs shadow-sm"
        style="top: {MARGIN.top}px; left: {tooltipFlip ? tooltipData.x - 144 : tooltipData.x + 8}px"
        aria-hidden="true"
      >
        {#each tooltipData.entries as entry (entry.label)}
          <div>{entry.label}: {entry.value}{entry.unit && ` ${entry.unit}`}</div>
        {/each}
      </div>
    {/if}

    <!-- Average pill tooltip -->
    {#if avgTooltip}
      <div
        data-testid="avg-tooltip"
        class="absolute pointer-events-none z-10 whitespace-nowrap rounded border border-hairline bg-surface px-2 py-1 text-xs shadow-sm"
        style="top: {avgTooltip.y - 8}px; left: {avgTooltip.left}px; transform: {avgTooltip.flip ? "translate(-100%, -100%)" : "translateY(-100%)"}"
        aria-hidden="true"
      >{avgTooltip.text}</div>
    {/if}
  {/if}
</div>

<!-- Legend -->
  <div class={legendClass} style={legendStyle}>
  {#each series as s (s.id)}
    {@const val = legendValue(s)}
    <button
      class="flex items-center gap-1 whitespace-nowrap rounded px-1.5 py-0.5 text-xs transition-opacity"
      class:opacity-40={hidden[s.id]}
      aria-pressed={!hidden[s.id]}
      onclick={() => toggleSeries(s.id)}
    >
      <span
        class="inline-block h-2 w-2 rounded-full"
        style="background: {s.color}"
        class:opacity-50={isSecondary(s)}
      ></span>
      <span class="text-zinc-500">{s.label}</span>
      {#if val}
        <span
          class="tabular-nums font-semibold text-zinc-100"
          class:min-w-[8ch]={!legendTight}
          class:text-right={!legendTight}
        >{val}</span>
      {/if}
    </button>
  {/each}
</div>
