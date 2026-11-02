import { describe, expect, it } from "vitest";
import { pushRing, pushRingSample, RING_CAP } from "../../src/lib/logic/ring";

describe("pushRing", () => {
  it("appends under capacity", () => {
    expect(pushRing([], 1)).toEqual([1]);
    expect(pushRing([1, 2], 3)).toEqual([1, 2, 3]);
  });

  it("evicts the oldest entry at capacity", () => {
    expect(pushRing([1, 2, 3], 4, 3)).toEqual([2, 3, 4]);
  });

  it("defaults to a 120-frame cap", () => {
    const full = Array.from({ length: RING_CAP }, (_, i) => i);
    const next = pushRing(full, 999);
    expect(next.length).toBe(RING_CAP);
    expect(next[0]).toBe(1);
    expect(next[RING_CAP - 1]).toBe(999);
  });
});

describe("pushRingSample (time-based)", () => {
  const NOW = 1_000_000;
  const WINDOW = 600_000;

  it("appends a sample within the window", () => {
    const ring = [{ ts_ms: NOW - 1000 }];
    const next = pushRingSample(ring, { ts_ms: NOW }, WINDOW);
    expect(next).toEqual([{ ts_ms: NOW - 1000 }, { ts_ms: NOW }]);
  });

  it("drops samples older than the window", () => {
    const ring = [{ ts_ms: NOW - WINDOW - 1000 }, { ts_ms: NOW - 1000 }];
    const next = pushRingSample(ring, { ts_ms: NOW }, WINDOW);
    expect(next).toEqual([{ ts_ms: NOW - 1000 }, { ts_ms: NOW }]);
  });

  it("drops all samples if all are outside the window", () => {
    const ring = [{ ts_ms: 0 }, { ts_ms: 100 }];
    const next = pushRingSample(ring, { ts_ms: NOW }, WINDOW);
    expect(next).toEqual([{ ts_ms: NOW }]);
  });

  it("handles empty ring", () => {
    const next = pushRingSample([], { ts_ms: NOW }, WINDOW);
    expect(next).toEqual([{ ts_ms: NOW }]);
  });
});
