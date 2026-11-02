import { describe, expect, it } from "vitest";
import {
  emptySlotForm,
  settingsFromSlotForm,
  slotFormFromSettings,
} from "../../src/lib/logic/form";
import type { PresetSettings } from "../../src/lib/types";

describe("settingsFromSlotForm", () => {
  it("empty form → all-null settings", () => {
    expect(settingsFromSlotForm(emptySlotForm())).toEqual({
      power_cap_w: null,
      sclk_offset_mhz: null,
      max_mclk_mhz: null,
      voltage_offset_mv: null,
    });
  });

  it("parses numbers", () => {
    const s = settingsFromSlotForm({
      ...emptySlotForm(),
      power_cap_w: "235",
      sclk_offset_mhz: "100",
    });
    expect(s.power_cap_w).toBe(235);
    expect(s.sclk_offset_mhz).toBe(100);
  });

  it("roundtrips through slotFormFromSettings", () => {
    const original: PresetSettings = {
      power_cap_w: 250,
      sclk_offset_mhz: 100,
      max_mclk_mhz: 1258,
      voltage_offset_mv: -20,
    };
    const f = slotFormFromSettings(original);
    expect(settingsFromSlotForm(f)).toEqual(original);
  });
});
