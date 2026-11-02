import { describe, expect, it } from "vitest";
import { pcieBand } from "../../src/lib/logic/pcie";

describe("pcieBand", () => {
  it("ok at the rig's normal state", () => {
    expect(pcieBand(16, 8.0)).toBe("ok");
  });

  it("ok when data is missing", () => {
    expect(pcieBand(null, null)).toBe("ok");
    expect(pcieBand(null, 8.0)).toBe("ok");
  });

  it("warn on width drop", () => {
    expect(pcieBand(8, 8.0)).toBe("warn");
    expect(pcieBand(4, 8.0)).toBe("warn");
  });

  it("warn on speed (gen) drop", () => {
    expect(pcieBand(16, 5.0)).toBe("warn");
    expect(pcieBand(16, 0)).toBe("warn");
  });

  it("warn when only one side is present and degraded", () => {
    expect(pcieBand(null, 5.0)).toBe("warn");
    expect(pcieBand(8, null)).toBe("warn");
  });
});
