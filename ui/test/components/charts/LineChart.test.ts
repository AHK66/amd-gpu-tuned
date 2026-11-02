import { render, fireEvent, waitFor } from "@testing-library/svelte";
import { describe, expect, it } from "vitest";
import { tick } from "svelte";
import LineChartHarness from "./LineChartHarness.svelte";
import type { LineSeries } from "../../../src/lib/components/charts/LineChart.svelte";

function makeSeries(overides: Partial<LineSeries> = {}): LineSeries {
  return {
    id: "s1",
    label: "GPU 0",
    color: "#38bdf8",
    points: [
      { t: 0, v: 50 },
      { t: 1000, v: 60 },
      { t: 2000, v: 70 },
    ],
    ...overides,
  };
}

async function renderChart(props: Record<string, unknown>) {
  const view = render(LineChartHarness, { props });
  await waitFor(() => expect(view.container.querySelector("svg")).not.toBeNull());
  return view;
}

describe("LineChart", () => {
  it("renders one path per series", async () => {
    const { container } = await renderChart({
      series: [makeSeries(), makeSeries({ id: "s2", label: "GPU 1" })],
    });
    const paths = container.querySelectorAll("svg path[data-series]");
    expect(paths.length).toBe(2);
  });

  it("splits a path at null values (gap)", async () => {
    const series: LineSeries[] = [
      {
        id: "s1",
        label: "GPU 0",
        color: "#38bdf8",
        points: [
          { t: 0, v: 50 },
          { t: 1000, v: null },
          { t: 2000, v: 70 },
        ],
      },
    ];
    const { container } = await renderChart({ series });
    const path = container.querySelector("svg path[data-series]") as SVGPathElement;
    const d = path?.getAttribute("d") ?? "";
    const moveToCount = (d.match(/M/g) ?? []).length;
    expect(moveToCount).toBeGreaterThanOrEqual(2);
  });

  it("renders y-axis tick labels with unit on top tick", async () => {
    const { container } = await renderChart({
      series: [makeSeries()],
      unit: "°C",
      yDomain: [0, 100],
    });
    const textEls = container.querySelectorAll("svg text");
    const texts = Array.from(textEls).map((t) => t.textContent ?? "");
    expect(texts.some((t) => t.includes("°C"))).toBe(true);
  });

  it("renders x-axis time labels", async () => {
    const series: LineSeries[] = [
      {
        id: "s1",
        label: "GPU 0",
        color: "#38bdf8",
        points: Array.from({ length: 60 }, (_, i) => ({
          t: i * 60_000,
          v: 50 + i,
        })),
      },
    ];
    const { container } = await renderChart({ series });
    const textEls = container.querySelectorAll("svg text");
    const hasTimeLabel = Array.from(textEls).some((t) =>
      /\d{2}:\d{2}/.test(t.textContent ?? ""),
    );
    expect(hasTimeLabel).toBe(true);
  });

  it("legend chip click hides the series path", async () => {
    const series: LineSeries[] = [makeSeries(), makeSeries({ id: "s2", label: "GPU 1" })];
    const { container } = await renderChart({ series });
    const chip = container.querySelector('button[aria-pressed="true"]') as HTMLButtonElement;
    expect(chip).not.toBeNull();
    await fireEvent.click(chip);
    const paths = container.querySelectorAll("svg path[data-series]");
    expect(paths.length).toBe(1);
  });

  it("shows tooltip on pointermove and hides on pointerleave", async () => {
    const { container } = await renderChart({ series: [makeSeries()] });
    const svg = container.querySelector("svg")!;
    svg.dispatchEvent(new MouseEvent("pointermove", { clientX: 100, clientY: 50, bubbles: true }));
    await tick();
    const tooltip = container.querySelector('[data-testid="chart-tooltip"]');
    expect(tooltip).not.toBeNull();
    svg.dispatchEvent(new MouseEvent("pointerleave"));
    await tick();
    const tooltipAfter = container.querySelector('[data-testid="chart-tooltip"]');
    expect(tooltipAfter).toBeNull();
  });

  it("shows empty state when no data", async () => {
    const { container } = await renderChart({ series: [] });
    expect(container.textContent).toContain("no data yet");
  });

  it("has role=img and aria-label on the SVG", async () => {
    const { container } = await renderChart({ series: [makeSeries()], title: "My Chart" });
    const svg = container.querySelector("svg");
    expect(svg?.getAttribute("role")).toBe("img");
    expect(svg?.getAttribute("aria-label")).toBe("My Chart");
  });

  it("renders thin dashed reference lines in-domain", async () => {
    const { container } = await renderChart({
      series: [makeSeries()],
      yDomain: [0, 100],
      references: [{ value: 80, label: "hotspot max", color: "#fbbf24" }],
    });
    const ref = container.querySelector("svg line[data-reference]");
    expect(ref).not.toBeNull();
    expect(ref?.getAttribute("stroke-dasharray")).toBeTruthy();
  });

  it("omits reference lines outside the y domain", async () => {
    const { container } = await renderChart({
      series: [makeSeries()],
      yDomain: [0, 100],
      references: [{ value: 500 }],
    });
    expect(container.querySelector("svg line[data-reference]")).toBeNull();
  });

  it("renders the voltages config: three solid mV series on a shared yDomain", async () => {
    const vgfx = makeSeries({
      id: "g-vgfx",
      label: "gfx",
      color: "#38bdf8",
      points: [{ t: 0, v: 900 }, { t: 1000, v: 950 }, { t: 2000, v: 1010 }],
    });
    const vsoc = makeSeries({
      id: "g-vsoc",
      label: "soc",
      color: "#60a5fa",
      points: [{ t: 0, v: 820 }, { t: 1000, v: 840 }, { t: 2000, v: 860 }],
    });
    const vmem = makeSeries({
      id: "g-vmem",
      label: "mem",
      color: "#8b5cf6",
      points: [{ t: 0, v: 1350 }, { t: 1000, v: 1380 }, { t: 2000, v: 1420 }],
    });
    const { container } = await renderChart({
      series: [vgfx, vsoc, vmem],
      unit: "mV",
      yDomain: [800, 1500],
    });
    expect(container.querySelectorAll("svg path[data-series]").length).toBe(3);
    for (const id of ["g-vgfx", "g-vsoc", "g-vmem"]) {
      const p = container.querySelector(`svg path[data-series="${id}"]`) as SVGPathElement;
      expect(p).not.toBeNull();
      expect(p?.getAttribute("stroke-dasharray")).toBeFalsy();
    }
    const texts = Array.from(container.querySelectorAll("svg text")).map(
      (t) => t.textContent ?? "",
    );
    expect(texts.some((t) => t.includes("mV"))).toBe(true);
  });

  it("places a reference label above the line by default", async () => {
    const { container } = await renderChart({
      series: [makeSeries()],
      yDomain: [0, 100],
      references: [{ value: 80, label: "hotspot max" }],
    });
    const line = container.querySelector(
      'svg line[data-reference="hotspot max"]',
    ) as SVGLineElement;
    const text = Array.from(container.querySelectorAll("svg text")).find(
      (t) => t.textContent?.trim() === "hotspot max",
    ) as SVGTextElement;
    expect(Number(text.getAttribute("y"))).toBeLessThan(Number(line.getAttribute("y1")));
  });

  it("places a reference label below the line when labelBelow is set", async () => {
    const { container } = await renderChart({
      series: [makeSeries()],
      yDomain: [0, 100],
      references: [{ value: 80, label: "vram max", labelBelow: true }],
    });
    const line = container.querySelector(
      'svg line[data-reference="vram max"]',
    ) as SVGLineElement;
    const text = Array.from(container.querySelectorAll("svg text")).find(
      (t) => t.textContent?.trim() === "vram max",
    ) as SVGTextElement;
    expect(Number(text.getAttribute("y"))).toBeGreaterThan(Number(line.getAttribute("y1")));
  });
});

describe("LineChart secondary (right) axis", () => {
  const power = () =>
    makeSeries({ id: "p", label: "power", color: "#3566f0" });
  const fan = (overides: Partial<LineSeries> = {}) =>
    makeSeries({
      id: "f",
      label: "fan",
      color: "#8b5cf6",
      points: [
        { t: 0, v: 1200 },
        { t: 1000, v: 3000 },
        { t: 2000, v: 4800 },
      ],
      ...overides,
    });

  it("renders a path per series including the secondary one", async () => {
    const { container } = await renderChart({
      series: [power(), fan()],
      unit: "W",
      secondary: { seriesIds: ["f"], unit: "rpm", yDomain: [0, 5000] },
    });
    expect(container.querySelectorAll("svg path[data-series]").length).toBe(2);
    expect(container.querySelector('svg path[data-series="f"]')).not.toBeNull();
  });

  it("renders right-axis tick labels with the secondary unit", async () => {
    const { container } = await renderChart({
      series: [power(), fan()],
      unit: "W",
      secondary: { seriesIds: ["f"], unit: "rpm", yDomain: [0, 5000] },
    });
    const texts = Array.from(container.querySelectorAll("svg text")).map(
      (t) => t.textContent ?? "",
    );
    expect(texts.some((t) => t.includes("rpm"))).toBe(true);
  });

  it("renders the secondary path muted (dashed)", async () => {
    const { container } = await renderChart({
      series: [power(), fan()],
      unit: "W",
      secondary: { seriesIds: ["f"], unit: "rpm", yDomain: [0, 5000] },
    });
    const fanPath = container.querySelector('svg path[data-series="f"]') as SVGPathElement;
    expect(fanPath?.getAttribute("stroke-dasharray")).toBeTruthy();
  });

  it("renders the clocks config: two solid MHz + two dashed % on [0,100]", async () => {
    const gfx = makeSeries({ id: "g-gfx", label: "gfx", color: "#38bdf8" });
    const mem = makeSeries({ id: "g-mem", label: "mem", color: "#8b5cf6" });
    const gutil = makeSeries({
      id: "g-gfxutil",
      label: "gfx %",
      color: "#38bdf8",
      points: [{ t: 0, v: 97 }, { t: 1000, v: 88 }, { t: 2000, v: 91 }],
    });
    const mutil = makeSeries({
      id: "g-memutil",
      label: "mem %",
      color: "#8b5cf6",
      points: [{ t: 0, v: 69 }, { t: 1000, v: 55 }, { t: 2000, v: 72 }],
    });
    const { container } = await renderChart({
      series: [gfx, mem, gutil, mutil],
      unit: "MHz",
      yDomain: [0, 3500],
      secondary: { seriesIds: ["g-gfxutil", "g-memutil"], unit: "%", yDomain: [0, 100] },
    });
    expect(container.querySelectorAll("svg path[data-series]").length).toBe(4);
    const gu = container.querySelector('svg path[data-series="g-gfxutil"]') as SVGPathElement;
    const mu = container.querySelector('svg path[data-series="g-memutil"]') as SVGPathElement;
    expect(gu?.getAttribute("stroke-dasharray")).toBeTruthy();
    expect(mu?.getAttribute("stroke-dasharray")).toBeTruthy();
    const texts = Array.from(container.querySelectorAll("svg text")).map((t) => t.textContent ?? "");
    expect(texts.some((t) => t.includes("%"))).toBe(true);
  });

  it("hides the right axis when the secondary series has no data", async () => {
    const { container } = await renderChart({
      series: [power(), fan({ points: [{ t: 0, v: null }, { t: 1000, v: null }] })],
      unit: "W",
      secondary: { seriesIds: ["f"], unit: "rpm", yDomain: [0, 5000] },
    });
    const texts = Array.from(container.querySelectorAll("svg text")).map(
      (t) => t.textContent ?? "",
    );
    expect(texts.some((t) => t.includes("rpm"))).toBe(false);
  });

  it("keeps single-axis behaviour when no secondary is configured", async () => {
    const { container } = await renderChart({
      series: [power()],
      unit: "W",
    });
    const texts = Array.from(container.querySelectorAll("svg text")).map(
      (t) => t.textContent ?? "",
    );
    expect(texts.some((t) => t.includes("rpm"))).toBe(false);
    expect(container.querySelectorAll("svg path[data-series]").length).toBe(1);
  });

  it("shows the current value with unit in each legend chip", async () => {
    const { container } = await renderChart({
      series: [power(), fan()],
      unit: "W",
      secondary: { seriesIds: ["f"], unit: "rpm", yDomain: [0, 5000] },
    });
    const chips = Array.from(container.querySelectorAll("button[aria-pressed]")).map(
      (b) => b.textContent ?? "",
    );
    // power's last point is 70 → "70 W"; fan's last point is 4800 → "4800 rpm"
    expect(chips.some((t) => t.includes("70 W"))).toBe(true);
    expect(chips.some((t) => t.includes("4800 rpm"))).toBe(true);
  });

  it("omits the legend value when showValue is false", async () => {
    const { container } = await renderChart({
      series: [power(), makeSeries({ id: "t", label: "edge", showValue: false })],
      unit: "W",
    });
    const chips = Array.from(container.querySelectorAll("button[aria-pressed]")).map(
      (b) => b.textContent ?? "",
    );
    const powerChip = chips.find((t) => t.includes("power"));
    const edgeChip = chips.find((t) => t.includes("edge"));
    // power (default) shows its value; edge (showValue:false) shows only the label
    expect(powerChip).toContain("70 W");
    expect(edgeChip).toBeDefined();
    expect(edgeChip).not.toMatch(/\d/);
  });

  it("lays legend chips out in a 2-column plot-aligned grid", async () => {
    const { container } = await renderChart({
      series: [
        makeSeries({ id: "a", label: "gfx util" }),
        makeSeries({ id: "b", label: "mem util" }),
        makeSeries({ id: "c", label: "gfx" }),
        makeSeries({ id: "d", label: "mem" }),
      ],
    });
    const legend = container.querySelector("div.grid") as HTMLElement;
    expect(legend).not.toBeNull();
    expect(legend.style.gridTemplateColumns).toBe("repeat(2, 1fr)");
    expect(legend.style.paddingLeft).toBe("56px");
    expect(legend.style.paddingRight).toBe("46px");
  });

  it("renders the legend value in a fixed-width right-aligned tabular slot", async () => {
    const { container } = await renderChart({
      series: [makeSeries()],
      unit: "W",
    });
    const val = container.querySelector(
      "button[aria-pressed] .tabular-nums",
    ) as HTMLElement;
    expect(val).not.toBeNull();
    expect(val.className).toContain("min-w-[8ch]");
    expect(val.className).toContain("text-right");
  });

  it("shows a dash (not a stale value) when the current value is not reported", async () => {
    const { container } = await renderChart({
      series: [
        makeSeries({
          id: "mem",
          label: "mem",
          points: [
            { t: 0, v: 96 },
            { t: 1000, v: 1258 },
            { t: 2000, v: null }, // latest sample not reported
          ],
        }),
      ],
      unit: "MHz",
    });
    const chip = Array.from(container.querySelectorAll("button[aria-pressed]")).find(
      (b) => (b.textContent ?? "").includes("mem"),
    );
    expect(chip).toBeDefined();
    expect(chip?.textContent).not.toMatch(/\d/);
    expect(chip?.textContent).toContain("—");
  });

  it("shows a per-entry unit in the tooltip (W and rpm)", async () => {
    const { container } = await renderChart({
      series: [power(), fan()],
      unit: "W",
      secondary: { seriesIds: ["f"], unit: "rpm", yDomain: [0, 5000] },
    });
    const svg = container.querySelector("svg")!;
    svg.dispatchEvent(
      new MouseEvent("pointermove", { clientX: 100, clientY: 50, bubbles: true }),
    );
    await tick();
    const tooltip = container.querySelector('[data-testid="chart-tooltip"]');
    expect(tooltip).not.toBeNull();
    const tooltipText = tooltip?.textContent ?? "";
    expect(tooltipText).toContain("W");
    expect(tooltipText).toContain("rpm");
  });

  it("shows min–max over the window for valueMode range", async () => {
    const { container } = await renderChart({
      series: [
        {
          id: "hs",
          label: "hotspot",
          color: "#fbbf24",
          valueMode: "range",
          points: [
            { t: 0, v: 62 },
            { t: 1000, v: 41 },
            { t: 2000, v: 83 },
          ],
        },
      ],
      unit: "°C",
    });
    const chips = Array.from(container.querySelectorAll("button[aria-pressed]")).map(
      (b) => b.textContent ?? "",
    );
    expect(chips[0]).toContain("41–83 °C");
  });

  it("range mode shows a dash when the series has no data", async () => {
    const { container } = await renderChart({
      series: [
        {
          id: "hs",
          label: "hotspot",
          color: "#fbbf24",
          valueMode: "range",
          points: [
            { t: 0, v: null },
            { t: 1000, v: null },
          ],
        },
      ],
      unit: "°C",
    });
    const chips = Array.from(container.querySelectorAll("button[aria-pressed]")).map(
      (b) => b.textContent ?? "",
    );
    expect(chips[0]).toContain("— °C");
  });

  it("range mode repeats the value when min == max", async () => {
    const { container } = await renderChart({
      series: [
        {
          id: "hs",
          label: "hotspot",
          color: "#fbbf24",
          valueMode: "range",
          points: [
            { t: 0, v: 41 },
            { t: 1000, v: 41 },
          ],
        },
      ],
      unit: "°C",
    });
    const chips = Array.from(container.querySelectorAll("button[aria-pressed]")).map(
      (b) => b.textContent ?? "",
    );
    expect(chips[0]).toContain("41–41 °C");
  });

  it("range mode honors decimals on both numbers", async () => {
    const { container } = await renderChart({
      series: [
        {
          id: "gb",
          label: "vram",
          color: "#8b5cf6",
          decimals: 1,
          valueMode: "range",
          points: [
            { t: 0, v: 26 },
            { t: 1000, v: 27.5 },
          ],
        },
      ],
      unit: "GB",
    });
    const chips = Array.from(container.querySelectorAll("button[aria-pressed]")).map(
      (b) => b.textContent ?? "",
    );
    expect(chips[0]).toContain("26.0–27.5 GB");
  });
});

describe("LineChart average markers", () => {
  const avgSeries = (overides: Partial<LineSeries> = {}) =>
    makeSeries({ avg: true, ...overides });

  it("renders a pill with the integer mean for an avg series", async () => {
    const { container } = await renderChart({
      series: [avgSeries()], // points 50, 60, 70 → mean 60
    });
    const pill = container.querySelector('g[data-avg-marker="s1"]');
    expect(pill).not.toBeNull();
    expect(pill?.querySelector("text")?.textContent).toBe("60");
  });

  it("rounds the mean to an integer in the pill text", async () => {
    const { container } = await renderChart({
      series: [
        avgSeries({
          points: [
            { t: 0, v: 61 },
            { t: 1000, v: 62 },
            { t: 2000, v: 62 },
          ], // mean 61.666…
        }),
      ],
    });
    expect(container.querySelector('g[data-avg-marker="s1"] text')?.textContent).toBe("62");
  });

  it("renders no pill for a series without avg", async () => {
    const { container } = await renderChart({ series: [makeSeries()] });
    expect(container.querySelector("g[data-avg-marker]")).toBeNull();
  });

  it("hides the pill when the series is legend-hidden", async () => {
    const { container } = await renderChart({ series: [avgSeries()] });
    expect(container.querySelector('g[data-avg-marker="s1"]')).not.toBeNull();
    const chip = container.querySelector('button[aria-pressed="true"]') as HTMLButtonElement;
    await fireEvent.click(chip);
    expect(container.querySelector('g[data-avg-marker="s1"]')).toBeNull();
  });

  it("renders no pill when every value is null", async () => {
    const { container } = await renderChart({
      series: [avgSeries({ points: [{ t: 0, v: null }, { t: 1000, v: null }] })],
    });
    expect(container.querySelector("g[data-avg-marker]")).toBeNull();
  });

  it("hides the pill when the mean falls outside a fixed yDomain", async () => {
    const { container } = await renderChart({
      series: [
        avgSeries({
          points: [
            { t: 0, v: 150 },
            { t: 1000, v: 152 },
            { t: 2000, v: 149 },
          ], // mean 150.33, outside [0, 100]
        }),
      ],
      yDomain: [0, 100],
    });
    expect(container.querySelector("g[data-avg-marker]")).toBeNull();
  });

  it("draws pills in avgOrder ascending so higher order ends on top (later in DOM)", async () => {
    const flat = (id: string, avgOrder: number, v: number): LineSeries => ({
      id,
      label: id,
      color: "#38bdf8",
      avg: true,
      avgOrder,
      points: [
        { t: 0, v },
        { t: 1000, v },
        { t: 2000, v },
      ],
    });
    // Deliberately passed out of order: b(2), a(1), c(3).
    const { container } = await renderChart({
      series: [flat("b", 2, 60), flat("a", 1, 60), flat("c", 3, 60)],
      yDomain: [0, 100],
    });
    const ids = Array.from(container.querySelectorAll("g[data-avg-marker]")).map(
      (g) => g.getAttribute("data-avg-marker"),
    );
    expect(ids).toEqual(["a", "b", "c"]);
  });
});

describe("LineChart axis-aware average markers", () => {
  const avg = (overides: Partial<LineSeries> = {}) => makeSeries({ avg: true, ...overides });

  it("renders a left-side pill for a primary avg series", async () => {
    const { container } = await renderChart({ series: [avg()] });
    const pill = container.querySelector('g[data-avg-marker="s1"]');
    expect(pill).not.toBeNull();
    expect(pill?.getAttribute("data-avg-side")).toBe("left");
  });

  it("renders a right-side pill for a secondary avg series", async () => {
    const { container } = await renderChart({
      series: [avg({ id: "gb" })],
      secondary: { seriesIds: ["gb"], unit: "GB", yDomain: [0, 100] },
    });
    const pill = container.querySelector('g[data-avg-marker="gb"]');
    expect(pill).not.toBeNull();
    expect(pill?.getAttribute("data-avg-side")).toBe("right");
  });

  it("auto-widens the pill to fit a four-digit mean", async () => {
    const four = avg({
      points: [
        { t: 0, v: 2979 },
        { t: 1000, v: 2979 },
      ],
    });
    const { container } = await renderChart({ series: [four], yDomain: [0, 3000] });
    const rect = container.querySelector('g[data-avg-marker="s1"] rect');
    expect(rect?.getAttribute("width")).toBe("36");
  });

  it("keeps a 24px pill for a two-digit mean", async () => {
    const { container } = await renderChart({ series: [avg()] }); // mean 60
    const rect = container.querySelector('g[data-avg-marker="s1"] rect');
    expect(rect?.getAttribute("width")).toBe("24");
  });

  it("positions the right pill to the right of the plot's right edge", async () => {
    const { container } = await renderChart({
      series: [avg({ id: "gb" })],
      secondary: { seriesIds: ["gb"], unit: "GB", yDomain: [0, 100] },
    });
    const svg = container.querySelector("svg")!;
    const width = Number(svg.getAttribute("width"));
    const rect = container.querySelector('g[data-avg-marker="gb"] rect')!;
    const x = Number(rect.getAttribute("x"));
    const w = Number(rect.getAttribute("width"));
    expect(x + w).toBeLessThanOrEqual(width); // pill fully inside the svg
    expect(x).toBeGreaterThan(width / 2); // pill on the right half
  });

  it("widens the right margin when a right-side pill is present", async () => {
    const { container } = await renderChart({
      series: [avg({ id: "gb" })],
      secondary: { seriesIds: ["gb"], unit: "GB", yDomain: [0, 100] },
    });
    const legend = container.querySelector("div.grid") as HTMLElement;
    // Base right margin is 46px; a right pill + its tick labels must push it wider.
    expect(parseFloat(legend.style.paddingRight)).toBeGreaterThan(46);
  });

  // NOTE: there is deliberately no dedicated right-side out-of-plot test. The
  // upper bound can't trigger: `yDom2` auto-expands a fixed `secondary.yDomain`
  // to `[lo, max(hi, dataMax)]` (LineChart.svelte:133-139), and a mean is always
  // <= dataMax. The lower bound *can* trigger (all data below `lo`), but the
  // guard expression is identical for both sides and already covered by the
  // existing left-side test "hides the pill when the mean falls outside a fixed
  // yDomain" (added by the thermals feature).

  it("stacks same-side pills by avgOrder ascending (higher order later in DOM)", async () => {
    const flat = (id: string, order: number, v: number): LineSeries => ({
      id,
      label: id,
      color: "#38bdf8",
      avg: true,
      avgOrder: order,
      points: [
        { t: 0, v },
        { t: 1000, v },
      ],
    });
    // Two left-side pills, deliberately out of order: b(2) then a(1).
    const { container } = await renderChart({ series: [flat("b", 2, 60), flat("a", 1, 60)] });
    const ids = Array.from(
      container.querySelectorAll("g[data-avg-side='left']"),
    ).map((g) => g.getAttribute("data-avg-marker"));
    expect(ids).toEqual(["a", "b"]);
  });

  it("shows a tooltip explaining the value is an average on pill hover", async () => {
    const { container } = await renderChart({ series: [avg()] }); // mean 60, label "GPU 0"
    const pill = container.querySelector('g[data-avg-marker="s1"]')!;
    expect(container.querySelector('[data-testid="avg-tooltip"]')).toBeNull();
    await fireEvent.pointerEnter(pill);
    const tip = container.querySelector('[data-testid="avg-tooltip"]');
    expect(tip).not.toBeNull();
    expect(tip?.textContent).toContain("average");
    expect(tip?.textContent).toContain("60");
    await fireEvent.pointerLeave(pill);
    expect(container.querySelector('[data-testid="avg-tooltip"]')).toBeNull();
  });
});
