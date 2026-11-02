import type { Band } from "./thermals";

export interface EccInput {
  ce: number | null;
  ue: number | null;
  deferred: number | null;
  ceNewInWindow: boolean; // ce now > ce at the oldest ring sample
}

export function eccCeBand(i: EccInput): Band {
  return i.ceNewInWindow ? "warn" : "ok";
}

export function eccUeBand(i: EccInput): Band {
  return (i.ue ?? 0) > 0 ? "critical" : "ok";
}

export function eccDeferredBand(i: EccInput): Band {
  return (i.deferred ?? 0) > 0 ? "warn" : "ok";
}
