import { describe, expect, it } from "vitest";
import { powerHeadroom } from "../../src/lib/logic/power";
import { sample } from "./thermals.test";

describe("powerHeadroom", () => {
  it("computes usage and headroom against the cap", () => {
    const h = powerHeadroom(sample({ socket_power_w: 250, power_cap_w: 300 }));
    expect(h?.headroom_w).toBe(50);
    expect(h?.used_pct).toBeCloseTo(83.3333, 3);
  });

  it("clamps usage to 100%", () => {
    expect(powerHeadroom(sample({ socket_power_w: 310, power_cap_w: 300 }))?.used_pct).toBe(100);
  });

  it("null without a cap", () => {
    expect(powerHeadroom(sample({ power_cap_w: null }))).toBeNull();
  });
});
