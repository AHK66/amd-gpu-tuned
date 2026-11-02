import type { GpuSample, GpuStatic } from "../types";

export type Sensor = "edge" | "hotspot" | "vram";
export type Band = "ok" | "warn" | "critical";

export interface Margin {
  sensor: Sensor;
  temp_c: number;
  threshold_c: number;
  margin_c: number;
}

export function distanceToSlowdown(sample: GpuSample, staticInfo: GpuStatic): Margin | null {
  const candidates: Margin[] = [];
  const push = (sensor: Sensor, temp: number | null, threshold: number | null) => {
    if (temp == null || threshold == null) return;
    candidates.push({ sensor, temp_c: temp, threshold_c: threshold, margin_c: threshold - temp });
  };
  push("edge", sample.edge_c, staticInfo.slowdown_edge_c);
  push("hotspot", sample.hotspot_c, staticInfo.slowdown_hotspot_c);
  push("vram", sample.vram_c, staticInfo.slowdown_vram_c);
  if (candidates.length === 0) return null;
  return candidates.reduce((a, b) => (b.margin_c < a.margin_c ? b : a));
}

export function bandForMargin(margin_c: number): Band {
  if (margin_c <= 5) return "critical";
  if (margin_c <= 15) return "warn";
  return "ok";
}

export const BAND_CLASS: Record<Band, string> = {
  ok: "text-success",
  warn: "text-warn",
  critical: "text-danger",
};

// Traffic-light band for how much VRAM is still free, as a fraction of the
// card total: plenty → ok, tightening → warn, nearly full → critical.
export function vramFreeBand(free_mb: number, total_mb: number): Band {
  const frac = total_mb > 0 ? free_mb / total_mb : 0;
  if (frac < 0.05) return "critical";
  if (frac < 0.2) return "warn";
  return "ok";
}

// GiB from MB, e.g. 32768 → "32GB", 5120 → "5.0GB", 512 → "0.5GB".
export function fmtGib(mb: number): string {
  const gb = mb / 1024;
  return gb >= 10 ? `${Math.round(gb)}GB` : `${gb.toFixed(1)}GB`;
}

export function isThrottled(sample: GpuSample): boolean {
  return (sample.throttle_bits?.length ?? 0) > 0;
}
