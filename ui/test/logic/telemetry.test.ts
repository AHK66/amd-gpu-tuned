import { describe, expect, it } from "vitest";
import { parseTelemetryFrame } from "../../src/lib/logic/telemetry";

describe("parseTelemetryFrame", () => {
  it("parses a valid frame", () => {
    const s = parseTelemetryFrame(JSON.stringify({ slot: "s", ts_ms: 1, socket_power_w: 200 }));
    expect(s?.slot).toBe("s");
  });

  it("rejects garbage and incomplete frames", () => {
    expect(parseTelemetryFrame("not json")).toBeNull();
    expect(parseTelemetryFrame(JSON.stringify({ ts_ms: 1 }))).toBeNull();
  });
});
