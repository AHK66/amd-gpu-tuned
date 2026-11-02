import { beforeEach, describe, expect, it } from "vitest";
import {
  discardAll,
  hasPending,
  pendingLabels,
  registerTile,
  resetForTests,
  saveAll,
  unregisterTile,
} from "../../src/lib/store/pending.svelte";

describe("pending store", () => {
  beforeEach(() => resetForTests());

  it("tracks registered tiles", () => {
    expect(hasPending()).toBe(false);
    registerTile("s1", { label: "a", save: async () => true, discard: async () => true });
    expect(hasPending()).toBe(true);
    expect(pendingLabels()).toEqual(["a"]);
    unregisterTile("s1");
    expect(hasPending()).toBe(false);
  });

  it("saveAll runs every tile's save and reports all-success", async () => {
    let order: string[] = [];
    registerTile("s1", { label: "a", save: async () => (order.push("s1"), true), discard: async () => true });
    registerTile("s2", { label: "b", save: async () => (order.push("s2"), true), discard: async () => true });
    await expect(saveAll()).resolves.toBe(true);
    expect(order).toEqual(["s1", "s2"]);
  });

  it("saveAll reports false if any tile fails", async () => {
    registerTile("s1", { label: "a", save: async () => true, discard: async () => true });
    registerTile("s2", { label: "b", save: async () => false, discard: async () => true });
    await expect(saveAll()).resolves.toBe(false);
  });

  it("discardAll runs every tile's discard and reports all-success", async () => {
    let n = 0;
    registerTile("s1", { label: "a", save: async () => true, discard: async () => (n++, true) });
    await expect(discardAll()).resolves.toBe(true);
    expect(n).toBe(1);
  });
});
