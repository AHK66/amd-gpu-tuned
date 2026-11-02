import { describe, expect, it } from "vitest";
import {
  firstViolation,
  knobRange,
  knobTooltip,
  violationMessage,
} from "../../src/lib/logic/knobs";
import { STATIC } from "./thermals.test";
import type { GpuStatic } from "../../src/lib/types";

const RANGED: GpuStatic = {
  ...STATIC,
  sclk_offset_range_mhz: [-500, 1000],
  mclk_range_mhz: [97, 1500],
  voltage_offset_range_mv: [-200, 0],
};

describe("knobRange", () => {
  it("returns the card-reported ranges", () => {
    expect(knobRange("sclk_offset_mhz", RANGED)).toEqual([-500, 1000]);
    expect(knobRange("max_mclk_mhz", RANGED)).toEqual([97, 1500]);
    expect(knobRange("voltage_offset_mv", RANGED)).toEqual([-200, 0]);
  });

  it("power cap comes from cap_min_w/cap_max_w", () => {
    expect(knobRange("power_cap_w", STATIC)).toEqual([50, 350]);
    expect(knobRange("power_cap_w", { ...STATIC, cap_min_w: null })).toBeNull();
  });

  it("returns null when the card reports no range, and for rangeless knobs", () => {
    expect(knobRange("sclk_offset_mhz", STATIC)).toBeNull();
  });
});

describe("knobTooltip", () => {
  it("returns the short phrase with the range when one exists", () => {
    expect(knobTooltip("power_cap_w", STATIC)).toBe("Power cap (50 .. 350 W)");
    expect(knobTooltip("sclk_offset_mhz", RANGED)).toBe(
      "Offset GPU core clock (-500 .. 1000 MHz)",
    );
    expect(knobTooltip("voltage_offset_mv", RANGED)).toBe(
      "Offset GPU core voltage (-200 .. 0 mV)",
    );
    expect(knobTooltip("max_mclk_mhz", RANGED)).toBe("Max VRAM clock (97 .. 1500 MHz)");
  });

  it("returns the short phrase without a range when none is reported", () => {
    expect(knobTooltip("sclk_offset_mhz", STATIC)).toBe("Offset GPU core clock");
  });
});

describe("firstViolation / violationMessage", () => {
  it("flags the first out-of-range value, in insertion order", () => {
    const v = firstViolation(
      { power_cap_w: 100, sclk_offset_mhz: 2000, max_mclk_mhz: 1500, voltage_offset_mv: -50 },
      RANGED,
    );
    expect(v).toEqual({ knob: "sclk_offset_mhz", value: 2000, lo: -500, hi: 1000 });
    expect(violationMessage(v!)).toBe("sclk offset 2000 out of range (-500…1000 MHz)");
  });

  it("boundary values are in range", () => {
    expect(
      firstViolation(
        { power_cap_w: 50, sclk_offset_mhz: -500, max_mclk_mhz: 1500, voltage_offset_mv: 0 },
        RANGED,
      ),
    ).toBeNull();
  });

  it("null (empty) values are skipped", () => {
    expect(firstViolation({ sclk_offset_mhz: null, max_mclk_mhz: null }, RANGED)).toBeNull();
  });

  it("skips validation when the card reports no range", () => {
    expect(firstViolation({ sclk_offset_mhz: 99999 }, STATIC)).toBeNull();
  });
});
