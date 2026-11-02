import { describe, expect, it } from "vitest";
import { bandForMargin, distanceToSlowdown, fmtGib, isThrottled, vramFreeBand } from "../../src/lib/logic/thermals";
import type { GpuSample, GpuStatic } from "../../src/lib/types";

export const STATIC: GpuStatic = {
  slot: "s",
  card_n: 0,
  vbios: null,
  slowdown_edge_c: 90,
  slowdown_hotspot_c: 110,
  slowdown_vram_c: 105,
  cap_min_w: 50,
  cap_max_w: 350,
  cap_default_w: 300,
  dpm_sclk_levels: [],
  dpm_mclk_levels: [],
  overdrive: true,
  sclk_offset_range_mhz: null,
  mclk_range_mhz: null,
  voltage_offset_range_mv: null,
  pmfw_fan: true,
  thermal_target: true,
  power_profile: true,
};

export function sample(p: Partial<GpuSample> = {}): GpuSample {
  return {
    slot: "s",
    ts_ms: 0,
    socket_power_w: 200,
    power_cap_w: 300,
    edge_c: null,
    hotspot_c: null,
    vram_c: null,
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
    ...p,
  };
}

describe("distanceToSlowdown", () => {
  it("returns the nearest sensor's margin", () => {
    const m = distanceToSlowdown(sample({ edge_c: 85, hotspot_c: 90, vram_c: 95 }), STATIC);
    expect(m).toEqual({ sensor: "edge", temp_c: 85, threshold_c: 90, margin_c: 5 });
  });

  it("ignores sensors without a threshold", () => {
    expect(distanceToSlowdown(sample({ edge_c: 50 }), { ...STATIC, slowdown_edge_c: null })).toBeNull();
  });

  it("handles at/over threshold", () => {
    expect(distanceToSlowdown(sample({ edge_c: 92 }), STATIC)?.margin_c).toBe(-2);
  });
});

describe("bandForMargin", () => {
  it("ok > 15, warn 5..15, critical <= 5", () => {
    expect(bandForMargin(30)).toBe("ok");
    expect(bandForMargin(15)).toBe("warn");
    expect(bandForMargin(6)).toBe("warn");
    expect(bandForMargin(5)).toBe("critical");
    expect(bandForMargin(-3)).toBe("critical");
  });
});

describe("isThrottled", () => {
  it("true only with non-empty throttle bits", () => {
    expect(isThrottled(sample())).toBe(false);
    expect(isThrottled(sample({ throttle_bits: [] }))).toBe(false);
    expect(isThrottled(sample({ throttle_bits: ["THROTTLE_THERMAL"] }))).toBe(true);
  });
});

describe("vramFreeBand", () => {
  it("ok >= 20%, warn 5..20%, critical < 5% of total", () => {
    expect(vramFreeBand(6554, 32768)).toBe("ok"); // just over 20%
    expect(vramFreeBand(6553, 32768)).toBe("warn"); // just under 20%
    expect(vramFreeBand(1639, 32768)).toBe("warn"); // just over 5%
    expect(vramFreeBand(1638, 32768)).toBe("critical"); // just under 5%
  });

  it("treats a zero total as critical", () => {
    expect(vramFreeBand(0, 0)).toBe("critical");
  });
});

describe("fmtGib", () => {
  it("integer GB at/over 10, one decimal below", () => {
    expect(fmtGib(32768)).toBe("32GB");
    expect(fmtGib(5120)).toBe("5.0GB");
    expect(fmtGib(512)).toBe("0.5GB");
  });
});
