import { describe, expect, it } from "vitest";
import {
  downsampleLTTB,
  nearestByTime,
  niceTicks,
  pillWidth,
  rightMarginFor,
  scaleLinear,
  seriesMean,
  sharedDomain,
  timeTicks,
} from "../../src/lib/logic/chart";

describe("scaleLinear", () => {
  it("maps domain endpoints to range endpoints", () => {
    const f = scaleLinear([0, 100], [0, 200]);
    expect(f(0)).toBe(0);
    expect(f(100)).toBe(200);
  });

  it("maps the midpoint", () => {
    const f = scaleLinear([0, 100], [0, 200]);
    expect(f(50)).toBe(100);
  });

  it("handles a single-value domain by mapping to range midpoint", () => {
    const f = scaleLinear([42, 42], [0, 200]);
    expect(f(42)).toBe(100);
  });

  it("handles inverted ranges", () => {
    const f = scaleLinear([0, 10], [100, 0]);
    expect(f(0)).toBe(100);
    expect(f(10)).toBe(0);
    expect(f(5)).toBe(50);
  });
});

describe("niceTicks", () => {
  it("produces step-5 ticks for (0, 19)", () => {
    const { ticks, domain } = niceTicks(0, 19);
    expect(domain[0]).toBeLessThanOrEqual(0);
    expect(domain[1]).toBeGreaterThanOrEqual(19);
    const step = ticks[1] - ticks[0];
    expect(step).toBe(5);
  });

  it("produces 40–90 range for (41.3, 87.9)", () => {
    const { domain } = niceTicks(41.3, 87.9);
    expect(domain[0]).toBe(40);
    expect(domain[1]).toBe(90);
  });

  it("expands equal min/max by max(1, |value|*0.01) in each direction", () => {
    const { domain } = niceTicks(50, 50);
    const pad = Math.max(1, 50 * 0.01);
    expect(domain[0]).toBeLessThanOrEqual(50 - pad);
    expect(domain[1]).toBeGreaterThanOrEqual(50 + pad);
  });

  it("handles large equal values", () => {
    const { domain } = niceTicks(100000, 100000);
    const pad = Math.max(1, 100000 * 0.01);
    expect(domain[0]).toBeLessThanOrEqual(100000 - pad);
    expect(domain[1]).toBeGreaterThanOrEqual(100000 + pad);
  });

  it("handles negative ranges", () => {
    const { ticks, domain } = niceTicks(-10, 10);
    expect(domain[0]).toBeLessThanOrEqual(-10);
    expect(domain[1]).toBeGreaterThanOrEqual(10);
    expect(ticks.length).toBeGreaterThanOrEqual(2);
  });

  it("honors count approximately", () => {
    const { ticks } = niceTicks(0, 1000, 10);
    expect(ticks.length).toBeGreaterThanOrEqual(8);
    expect(ticks.length).toBeLessThanOrEqual(14);
  });

  it("always includes at least 2 ticks", () => {
    const { ticks } = niceTicks(0, 1);
    expect(ticks.length).toBeGreaterThanOrEqual(2);
  });
});

describe("timeTicks", () => {
  const MIN = 60_000;

  it("snaps to 5-min boundaries for a 30-min span", () => {
    const t0 = 0;
    const t1 = 30 * MIN;
    const ticks = timeTicks(t0, t1);
    for (const t of ticks) {
      expect(t % (5 * MIN)).toBe(0);
    }
  });

  it("produces 10-min steps for a 1-hour span", () => {
    const t0 = 0;
    const t1 = 60 * MIN;
    const ticks = timeTicks(t0, t1);
    if (ticks.length >= 2) {
      const step = ticks[1] - ticks[0];
      expect(step).toBe(10 * MIN);
    }
  });

  it("returns an empty array for an empty span", () => {
    expect(timeTicks(1000, 1000)).toEqual([]);
  });

  it("returns ticks within [t0, t1]", () => {
    const t0 = 5 * MIN;
    const t1 = 25 * MIN;
    const ticks = timeTicks(t0, t1);
    for (const t of ticks) {
      expect(t).toBeGreaterThanOrEqual(t0);
      expect(t).toBeLessThanOrEqual(t1);
    }
  });
});

describe("downsampleLTTB", () => {
  it("passes through when points.length <= threshold", () => {
    const pts = [{ t: 0, v: 1 }, { t: 1, v: 2 }, { t: 2, v: 3 }];
    expect(downsampleLTTB(pts, 10)).toBe(pts);
  });

  it("returns exactly threshold points when downsampling", () => {
    const pts = Array.from({ length: 100 }, (_, i) => ({ t: i, v: Math.sin(i) }));
    const out = downsampleLTTB(pts, 20);
    expect(out.length).toBe(20);
  });

  it("preserves first and last points", () => {
    const pts = Array.from({ length: 100 }, (_, i) => ({ t: i, v: i * 2 }));
    const out = downsampleLTTB(pts, 10);
    expect(out[0]).toEqual(pts[0]);
    expect(out[out.length - 1]).toEqual(pts[pts.length - 1]);
  });
});

describe("nearestByTime", () => {
  it("finds an exact match", () => {
    const pts = [{ t: 0 }, { t: 100 }, { t: 200 }];
    expect(nearestByTime(pts, 100)).toBe(1);
  });

  it("finds the nearest between points", () => {
    const pts = [{ t: 0 }, { t: 100 }, { t: 200 }];
    expect(nearestByTime(pts, 60)).toBe(1);
    expect(nearestByTime(pts, 30)).toBe(0);
  });

  it("returns -1 for empty input", () => {
    expect(nearestByTime([], 500)).toBe(-1);
  });
});

describe("sharedDomain", () => {
  it("returns undefined for empty or all-null input", () => {
    expect(sharedDomain([])).toBeUndefined();
    expect(sharedDomain([null, null])).toBeUndefined();
  });

  it("covers the global min/max across per-GPU values with nice bounds", () => {
    const d = sharedDomain([920, null, 1450, 875, null, 1490]);
    expect(d).toBeDefined();
    expect(d![0]).toBeLessThanOrEqual(875);
    expect(d![1]).toBeGreaterThanOrEqual(1490);
    expect(d).toEqual(niceTicks(875, 1490).domain);
  });

  it("is idempotent on a constant input set", () => {
    const d1 = sharedDomain([1000, 1000, null]);
    const d2 = sharedDomain([d1![0], d1![1], null]);
    expect(d2).toEqual(d1);
  });
});

describe("seriesMean", () => {
  it("averages the non-null values", () => {
    expect(
      seriesMean([
        { t: 0, v: 50 },
        { t: 1000, v: 60 },
        { t: 2000, v: 70 },
      ]),
    ).toBe(60);
  });

  it("skips null values", () => {
    expect(
      seriesMean([
        { t: 0, v: 40 },
        { t: 1000, v: null },
        { t: 2000, v: 80 },
      ]),
    ).toBe(60);
  });

  it("returns a single point's value", () => {
    expect(seriesMean([{ t: 0, v: 41.5 }])).toBe(41.5);
  });

  it("returns null when every value is null", () => {
    expect(seriesMean([{ t: 0, v: null }, { t: 1000, v: null }])).toBeNull();
  });

  it("returns null for an empty list", () => {
    expect(seriesMean([])).toBeNull();
  });
});

describe("pillWidth", () => {
  it("is 24px for one- and two-digit values", () => {
    expect(pillWidth(0)).toBe(24);
    expect(pillWidth(78)).toBe(24);
    expect(pillWidth(99)).toBe(24);
  });

  it("grows to 30px for three digits", () => {
    expect(pillWidth(100)).toBe(30);
    expect(pillWidth(999)).toBe(30);
  });

  it("grows to 36px for four digits", () => {
    expect(pillWidth(1100)).toBe(36);
    expect(pillWidth(2979)).toBe(36);
  });
});

describe("rightMarginFor", () => {
  it("returns the base when there is no right pill", () => {
    expect(rightMarginFor({ widestPill: 0, tickLabelWidth: 30 })).toBe(46);
  });

  it("widens to fit the pill plus its tick labels", () => {
    // 6 + 20 + 4 + 36 = 66
    expect(rightMarginFor({ widestPill: 36, tickLabelWidth: 20 })).toBe(66);
  });

  it("never shrinks below the base", () => {
    // 6 + 12 + 4 + 24 = 46 → base
    expect(rightMarginFor({ widestPill: 24, tickLabelWidth: 12 })).toBe(46);
  });

  it("honours a custom base", () => {
    // 6 + 20 + 4 + 36 = 66 > 40
    expect(rightMarginFor({ widestPill: 36, tickLabelWidth: 20, base: 40 })).toBe(66);
  });
});
