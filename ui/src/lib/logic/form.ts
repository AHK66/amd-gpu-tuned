import type { PresetSettings } from "../types";

export interface SlotForm {
  power_cap_w: string;
  sclk_offset_mhz: string;
  max_mclk_mhz: string;
  voltage_offset_mv: string;
}

export function emptySlotForm(): SlotForm {
  return {
    power_cap_w: "",
    sclk_offset_mhz: "",
    max_mclk_mhz: "",
    voltage_offset_mv: "",
  };
}

function num(s: string): number | null {
  const t = s.trim();
  if (t === "") return null;
  const n = Number(t);
  return Number.isFinite(n) ? n : null;
}

export function slotFormFromSettings(s: PresetSettings | undefined): SlotForm {
  const f = emptySlotForm();
  if (!s) return f;
  f.power_cap_w = s.power_cap_w != null ? String(s.power_cap_w) : "";
  f.sclk_offset_mhz = s.sclk_offset_mhz != null ? String(s.sclk_offset_mhz) : "";
  f.max_mclk_mhz = s.max_mclk_mhz != null ? String(s.max_mclk_mhz) : "";
  f.voltage_offset_mv = s.voltage_offset_mv != null ? String(s.voltage_offset_mv) : "";
  return f;
}

export function settingsFromSlotForm(f: SlotForm): PresetSettings {
  return {
    power_cap_w: num(f.power_cap_w),
    sclk_offset_mhz: num(f.sclk_offset_mhz),
    max_mclk_mhz: num(f.max_mclk_mhz),
    voltage_offset_mv: num(f.voltage_offset_mv),
  };
}
