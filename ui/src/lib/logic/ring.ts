export const RING_CAP = 120;

export function pushRing<T>(ring: readonly T[], value: T, cap: number = RING_CAP): T[] {
  const next: T[] = ring.length >= cap ? ring.slice(ring.length - cap + 1) : ring.slice();
  next.push(value);
  return next;
}

export function pushRingSample<T extends { ts_ms: number }>(
  ring: readonly T[],
  sample: T,
  windowMs: number,
): T[] {
  const cutoff = sample.ts_ms - windowMs;
  const kept = ring.filter((s) => s.ts_ms >= cutoff);
  kept.push(sample);
  return kept;
}
