import { describe, expect, it } from "vitest";
import { makeLatestOnly } from "../../src/lib/logic/latest";

describe("makeLatestOnly", () => {
  it("drops a stale response that resolves after a newer fetch", async () => {
    const applied: string[] = [];
    const set = makeLatestOnly<string>((v) => applied.push(v));

    let resolveStale!: (v: string) => void;
    const stale = new Promise<string>((r) => (resolveStale = r));

    // Fetch #1 starts first but resolves last (a slow, stale poll).
    void set(() => stale);
    // Fetch #2 starts after and resolves first (a fresh read).
    await set(() => Promise.resolve("fresh"));
    // The stale response finally arrives.
    resolveStale("stale");
    await new Promise((r) => setTimeout(r, 0));

    // Only the fresh value may be applied; the stale one is dropped.
    expect(applied).toEqual(["fresh"]);
  });

  it("applies every result when fetches do not overlap", async () => {
    const applied: string[] = [];
    const set = makeLatestOnly<string>((v) => applied.push(v));
    await set(() => Promise.resolve("a"));
    await set(() => Promise.resolve("b"));
    expect(applied).toEqual(["a", "b"]);
  });

  it("does not apply a stale failure after a newer success", async () => {
    const applied: string[] = [];
    let threw = false;
    const set = makeLatestOnly<string>((v) => applied.push(v));

    let rejectStale!: (e: Error) => void;
    const stale = new Promise<string>((_, r) => (rejectStale = r));

    void set(() => stale);
    await set(() => Promise.resolve("fresh"));
    rejectStale(new Error("stale failed"));
    await new Promise((r) => setTimeout(r, 0)).catch(() => {
      threw = true;
    });

    expect(applied).toEqual(["fresh"]);
    expect(threw).toBe(false);
  });
});
