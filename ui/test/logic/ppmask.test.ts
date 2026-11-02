import { describe, expect, it } from "vitest";
import { enabledFeatureCount } from "../../src/lib/logic/ppmask";

describe("enabledFeatureCount", () => {
  it("counts set bits in a 32-bit mask", () => {
    expect(enabledFeatureCount(0xfff7ffff)).toBe(31);
  });
  it("returns 0 for an empty mask", () => {
    expect(enabledFeatureCount(0)).toBe(0);
  });
  it("returns 32 for an all-ones 32-bit mask", () => {
    expect(enabledFeatureCount(0xffffffff)).toBe(32);
  });
  it("counts multiple scattered bits", () => {
    expect(enabledFeatureCount(0b101)).toBe(2);
  });
});
