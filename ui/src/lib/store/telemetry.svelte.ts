import { pushRingSample } from "../logic/ring";
import { parseTelemetryFrame } from "../logic/telemetry";
import type { GpuSample } from "../types";

export const WINDOW_MS = 600_000;

export const samples = $state<Record<string, GpuSample>>({});
export const rings = $state<Record<string, GpuSample[]>>({});
// Plain exported `let` (ESM live binding), NOT $state: Svelte 5 forbids
// exporting $state that is reassigned from a module (state_invalid_export),
// and `export get` is not parseable here because esbuild strips TS types
// from .svelte.ts before Svelte compiles it. Consumers reading `feedDown`
// see the current value on any re-render (e.g. the per-second `samples`
// updates); it is not independently reactive.
export let feedDown = false;

export function onSseMessage(e: MessageEvent): void {
  const s = parseTelemetryFrame(String(e.data));
  if (!s) return;
  samples[s.slot] = s;
  rings[s.slot] = pushRingSample(rings[s.slot] ?? [], s, WINDOW_MS);
  feedDown = false;
}

export function onSseError(): void {
  feedDown = true;
}

export function seedFromRing(ring: GpuSample[]): void {
  for (const s of ring) {
    samples[s.slot] = s;
  }
  // Idempotent merge per slot: union the fetched server ring with the existing
  // buffer by timestamp, keep time order, trim to the window. The buffer
  // persists across route changes and is live-fed by SSE, so a Dashboard
  // re-mount re-seeds it. Appending (the old behavior) duplicated points and
  // made the series run forward, jump back to the start, and run forward
  // again — which the chart drew as a spurious full-width line.
  for (const slot of new Set(ring.map((s) => s.slot))) {
    const existing = rings[slot] ?? [];
    const seen = new Set(existing.map((e) => e.ts_ms));
    const merged = existing.slice();
    for (const s of ring) {
      if (s.slot === slot && !seen.has(s.ts_ms)) {
        merged.push(s);
        seen.add(s.ts_ms);
      }
    }
    merged.sort((a, b) => a.ts_ms - b.ts_ms);
    const newest = merged[merged.length - 1]?.ts_ms;
    const cutoff = newest !== undefined ? newest - WINDOW_MS : 0;
    rings[slot] = merged.filter((s) => s.ts_ms >= cutoff);
  }
}

export function resetForTests(): void {
  for (const k of Object.keys(samples)) delete samples[k];
  for (const k of Object.keys(rings)) delete rings[k];
  feedDown = false;
}
