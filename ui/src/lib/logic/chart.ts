export function scaleLinear(
  domain: [number, number],
  range: [number, number],
): (v: number) => number {
  const [d0, d1] = domain;
  const [r0, r1] = range;
  if (d0 === d1) return () => (r0 + r1) / 2;
  const slope = (r1 - r0) / (d1 - d0);
  return (v: number) => r0 + (v - d0) * slope;
}

export function niceTicks(
  min: number,
  max: number,
  count = 5,
): { ticks: number[]; domain: [number, number] } {
  if (min === max) {
    const pad = Math.max(1, Math.abs(min) * 0.01);
    min -= pad;
    max += pad;
  }
  const rawStep = (max - min) / count;
  const mag = Math.pow(10, Math.floor(Math.log10(rawStep)));
  const residual = rawStep / mag;
  let niceStep: number;
  if (residual <= 1.5) niceStep = 1 * mag;
  else if (residual <= 3.5) niceStep = 2 * mag;
  else if (residual <= 7.5) niceStep = 5 * mag;
  else niceStep = 10 * mag;

  const niceMin = Math.floor(min / niceStep) * niceStep;
  const niceMax = Math.ceil(max / niceStep) * niceStep;

  const ticks: number[] = [];
  for (let v = niceMin; v <= niceMax + niceStep * 0.5; v += niceStep) {
    ticks.push(Math.round(v * 1e10) / 1e10);
  }
  return { ticks, domain: [niceMin, niceMax] };
}

export function timeTicks(t0: number, t1: number, count = 6): number[] {
  if (t1 <= t0) return [];
  const span = t1 - t0;
  const MIN = 60_000;
  const steps = [MIN, 5 * MIN, 10 * MIN, 30 * MIN, 60 * MIN];
  let step = steps[steps.length - 1];
  for (const s of steps) {
    if (span / s <= count) {
      step = s;
      break;
    }
  }
  const first = Math.ceil(t0 / step) * step;
  const ticks: number[] = [];
  for (let t = first; t <= t1; t += step) {
    ticks.push(t);
  }
  return ticks;
}

export function downsampleLTTB(
  points: { t: number; v: number }[],
  threshold: number,
): { t: number; v: number }[] {
  const n = points.length;
  if (n <= threshold) return points;

  const sampled: { t: number; v: number }[] = [points[0]];
  const bucketSize = (n - 2) / (threshold - 2);

  let a = 0;
  for (let i = 0; i < threshold - 2; i++) {
    const bucketStart = Math.floor((i + 1) * bucketSize) + 1;
    const bucketEnd = Math.min(Math.floor((i + 2) * bucketSize) + 1, n - 1);

    const bucketLen = bucketEnd - bucketStart;
    let avgT: number;
    let avgV: number;
    if (bucketLen === 0) {
      avgT = points[n - 1].t;
      avgV = points[n - 1].v;
    } else {
      let sumT = 0;
      let sumV = 0;
      for (let j = bucketStart; j < bucketEnd; j++) {
        sumT += points[j].t;
        sumV += points[j].v;
      }
      avgT = sumT / bucketLen;
      avgV = sumV / bucketLen;
    }

    const rangeStart = Math.floor(i * bucketSize) + 1;
    const rangeEnd = Math.min(Math.floor((i + 1) * bucketSize) + 1, n - 1);

    let maxArea = -1;
    let maxIdx = rangeStart;
    const ax = points[a].t;
    const ay = points[a].v;

    for (let j = rangeStart; j < rangeEnd; j++) {
      const area = Math.abs(
        (ax - avgT) * (points[j].v - ay) - (ax - points[j].t) * (avgV - ay),
      );
      if (area > maxArea) {
        maxArea = area;
        maxIdx = j;
      }
    }
    sampled.push(points[maxIdx]);
    a = maxIdx;
  }
  sampled.push(points[n - 1]);
  return sampled;
}

export function nearestByTime(points: { t: number }[], t: number): number {
  if (points.length === 0) return -1;
  let lo = 0;
  let hi = points.length - 1;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (points[mid].t < t) lo = mid + 1;
    else hi = mid;
  }
  if (lo > 0 && Math.abs(points[lo - 1].t - t) < Math.abs(points[lo].t - t)) {
    return lo - 1;
  }
  return lo;
}

export function sharedDomain(
  values: Iterable<number | null>,
): [number, number] | undefined {
  const vals = [...values].filter((v): v is number => v != null);
  if (vals.length === 0) return undefined;
  let min = Infinity;
  let max = -Infinity;
  for (const v of vals) {
    if (v < min) min = v;
    if (v > max) max = v;
  }
  return niceTicks(min, max).domain;
}

export function seriesMean(points: { t: number; v: number | null }[]): number | null {
  let sum = 0;
  let n = 0;
  for (const p of points) {
    if (p.v !== null) {
      sum += p.v;
      n += 1;
    }
  }
  return n === 0 ? null : sum / n;
}

export function pillWidth(value: number): number {
  const digits = String(Math.abs(Math.round(value))).length;
  return Math.max(24, 12 + digits * 6);
}

export function rightMarginFor(opts: {
  widestPill: number;
  tickLabelWidth: number;
  base?: number;
}): number {
  const base = opts.base ?? 46;
  if (opts.widestPill <= 0) return base;
  return Math.max(base, 6 + opts.tickLabelWidth + 4 + opts.widestPill);
}
