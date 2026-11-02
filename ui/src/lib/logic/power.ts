import type { GpuSample } from "../types";

export interface Headroom {
  used_pct: number;
  headroom_w: number;
}

export function powerHeadroom(sample: GpuSample): Headroom | null {
  const cap = sample.power_cap_w;
  if (cap == null || cap <= 0) return null;
  return {
    used_pct: Math.max(0, Math.min(100, (sample.socket_power_w / cap) * 100)),
    headroom_w: cap - sample.socket_power_w,
  };
}
