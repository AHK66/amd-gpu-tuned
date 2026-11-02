import type { GpuStatic } from "../types";

export type KnobId =
  | "power_cap_w"
  | "sclk_offset_mhz"
  | "voltage_offset_mv"
  | "max_mclk_mhz";

interface Meta {
  label: string;
  short: string;
  unit: string;
}

const META: Record<KnobId, Meta> = {
  power_cap_w: { label: "power cap", short: "Power cap", unit: "W" },
  sclk_offset_mhz: { label: "sclk offset", short: "Offset GPU core clock", unit: "MHz" },
  voltage_offset_mv: { label: "voltage offset", short: "Offset GPU core voltage", unit: "mV" },
  max_mclk_mhz: { label: "max mclk", short: "Max VRAM clock", unit: "MHz" },
};

export function knobRange(knob: KnobId, s: GpuStatic): [number, number] | null {
  switch (knob) {
    case "power_cap_w":
      return s.cap_min_w != null && s.cap_max_w != null ? [s.cap_min_w, s.cap_max_w] : null;
    case "sclk_offset_mhz":
      return s.sclk_offset_range_mhz;
    case "voltage_offset_mv":
      return s.voltage_offset_range_mv;
    case "max_mclk_mhz":
      return s.mclk_range_mhz;
    default:
      return null;
  }
}

export function knobTooltip(knob: KnobId, s: GpuStatic): string {
  const { short, unit } = META[knob];
  const range = knobRange(knob, s);
  if (range == null) return short;
  const u = unit === "" ? "" : ` ${unit}`;
  return `${short} (${range[0]} .. ${range[1]}${u})`;
}

export interface Violation {
  knob: KnobId;
  value: number;
  lo: number;
  hi: number;
}

export function firstViolation(
  values: Partial<Record<KnobId, number | null>>,
  s: GpuStatic,
): Violation | null {
  for (const [knob, value] of Object.entries(values) as [KnobId, number | null][]) {
    if (value == null) continue;
    const range = knobRange(knob, s);
    if (range != null && (value < range[0] || value > range[1])) {
      return { knob, value, lo: range[0], hi: range[1] };
    }
  }
  return null;
}

export function violationMessage(v: Violation): string {
  const { label, unit } = META[v.knob];
  const u = unit === "" ? "" : ` ${unit}`;
  return `${label} ${v.value} out of range (${v.lo}…${v.hi}${u})`;
}
