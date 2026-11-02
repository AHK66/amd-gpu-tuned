import { describe, expect, it } from "vitest";
import { eccCeBand, eccDeferredBand, eccUeBand, type EccInput } from "../../src/lib/logic/ecc";

const base: EccInput = { ce: 0, ue: 0, deferred: 0, ceNewInWindow: false };

describe("eccUeBand", () => {
  it("ok at 0, critical above 0", () => {
    expect(eccUeBand(base)).toBe("ok");
    expect(eccUeBand({ ...base, ue: 1 })).toBe("critical");
  });
  it("treats a null ue as ok", () => {
    expect(eccUeBand({ ...base, ue: null })).toBe("ok");
  });
});

describe("eccDeferredBand", () => {
  it("ok at 0, warn above 0", () => {
    expect(eccDeferredBand(base)).toBe("ok");
    expect(eccDeferredBand({ ...base, deferred: 1 })).toBe("warn");
  });
  it("treats a null deferred as ok", () => {
    expect(eccDeferredBand({ ...base, deferred: null })).toBe("ok");
  });
});

describe("eccCeBand", () => {
  it("warn when new in window, ok otherwise", () => {
    expect(eccCeBand(base)).toBe("ok");
    expect(eccCeBand({ ...base, ceNewInWindow: true })).toBe("warn");
  });
});
