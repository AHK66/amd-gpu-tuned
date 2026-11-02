import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  feedDown,
  onSseError,
  onSseMessage,
  resetForTests,
  rings,
  samples,
  seedFromRing,
  WINDOW_MS,
} from "../../src/lib/store/telemetry.svelte";
import type { GpuSample } from "../../src/lib/types";

const FRAME = JSON.stringify({ slot: "s1", ts_ms: 1, socket_power_w: 200, power_cap_w: 300 });

function makeSample(slot: string, ts: number, power = 100): GpuSample {
  return {
    slot,
    ts_ms: ts,
    socket_power_w: power,
    power_cap_w: 300,
    edge_c: 50,
    hotspot_c: 55,
    vram_c: 45,
    gfx_mhz: null,
    mem_mhz: null,
    gfx_activity_pct: null,
    mem_activity_pct: null,
    vram_used_mb: null,
    vram_total_mb: null,
    dpm_sclk_mhz: null,
    dpm_mclk_mhz: null,
    throttle_bits: null,
    fan_rpm: null,
    fan_pwm_pct: null,
    voltage_gfx_mv: null,
    voltage_soc_mv: null,
    voltage_mem_mv: null,
    ecc_umc_ce: null,
    ecc_umc_ue: null,
    ecc_umc_deferred: null,
    pcie_width: null,
    pcie_speed_gt: null,
  };
}

describe("telemetry store", () => {
  beforeEach(() => resetForTests());

  it("stores samples and grows the per-slot ring", () => {
    onSseMessage({ data: FRAME } as MessageEvent);
    expect(samples["s1"].socket_power_w).toBe(200);
    expect(rings["s1"].length).toBe(1);
    onSseMessage({ data: FRAME } as MessageEvent);
    expect(rings["s1"].length).toBe(2);
  });

  it("ignores malformed frames", () => {
    onSseMessage({ data: "garbage" } as MessageEvent);
    expect(Object.keys(samples)).toEqual([]);
  });

  it("flags the feed down on error and recovers", () => {
    onSseError();
    expect(feedDown).toBe(true);
    onSseMessage({ data: FRAME } as MessageEvent);
    expect(feedDown).toBe(false);
  });
});

describe("seedFromRing", () => {
  beforeEach(() => resetForTests());

  it("splits a mixed-slot ring into per-slot buffers", () => {
    const ring: GpuSample[] = [
      makeSample("s1", 1000),
      makeSample("s2", 1000),
      makeSample("s1", 2000),
      makeSample("s2", 2000),
    ];
    seedFromRing(ring);
    expect(rings["s1"].length).toBe(2);
    expect(rings["s2"].length).toBe(2);
    expect(rings["s1"][0].ts_ms).toBe(1000);
    expect(rings["s1"][1].ts_ms).toBe(2000);
  });

  it("sets the latest sample per slot", () => {
    const ring: GpuSample[] = [makeSample("s1", 1000, 50), makeSample("s1", 2000, 150)];
    seedFromRing(ring);
    expect(samples["s1"].socket_power_w).toBe(150);
  });

  it("does not drop samples within the window", () => {
    const now = Date.now();
    const ring: GpuSample[] = [
      makeSample("s1", now - WINDOW_MS + 1000),
      makeSample("s1", now),
    ];
    seedFromRing(ring);
    expect(rings["s1"].length).toBe(2);
  });

  it("is idempotent: re-seeding an overlapping ring does not duplicate points", () => {
    // The per-slot buffer persists across route changes and is live-fed by SSE,
    // so Dashboard re-mounts re-seed from the server ring. That must merge, not
    // append — duplicated timestamps make the series run forward, jump back to
    // the start, and run forward again, which the chart draws as a spurious
    // full-width line.
    const base: GpuSample[] = [
      makeSample("s1", 1000),
      makeSample("s1", 2000),
      makeSample("s1", 3000),
    ];
    seedFromRing(base);
    // A live SSE sample arrives with a newer timestamp, then a re-mount re-seeds
    // the same server ring (now including that sample).
    seedFromRing([...base, makeSample("s1", 4000)]);

    const ts = rings["s1"].map((s) => s.ts_ms);
    expect(new Set(ts).size).toBe(ts.length); // no duplicate timestamps
    expect(ts).toEqual([...ts].sort((a, b) => a - b)); // time-ordered
    expect(ts).toEqual([1000, 2000, 3000, 4000]);
  });
});

describe("time-based window in onSseMessage", () => {
  beforeEach(() => resetForTests());

  it("drops samples older than WINDOW_MS", () => {
    const now = Date.now();
    const oldFrame = JSON.stringify(makeSample("s1", now - WINDOW_MS - 10_000));
    const newFrame = JSON.stringify(makeSample("s1", now));
    onSseMessage({ data: oldFrame } as MessageEvent);
    onSseMessage({ data: newFrame } as MessageEvent);
    expect(rings["s1"].every((s) => s.ts_ms >= now - WINDOW_MS)).toBe(true);
  });
});
