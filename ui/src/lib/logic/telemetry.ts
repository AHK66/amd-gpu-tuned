import type { GpuSample } from "../types";

export function parseTelemetryFrame(data: string): GpuSample | null {
  try {
    const v = JSON.parse(data) as GpuSample;
    if (typeof v?.slot !== "string" || typeof v?.socket_power_w !== "number") return null;
    return v;
  } catch {
    return null;
  }
}
