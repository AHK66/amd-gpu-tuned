import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  blockedRoute,
  navigate,
  parseRoute,
  proceed,
  resetForTests,
  setNavGuard,
} from "../src/lib/router.svelte";

describe("parseRoute", () => {
  it("maps hashes to routes", () => {
    expect(parseRoute("#/")).toBe("");
    expect(parseRoute("#/presets")).toBe("presets");
    expect(parseRoute("#/drift")).toBe("drift");
    expect(parseRoute("#/setup")).toBe("setup");
  });

  it("unknown or empty hash → dashboard", () => {
    expect(parseRoute("#/nope")).toBe("");
    expect(parseRoute("")).toBe("");
  });
});

describe("nav guard", () => {
  beforeEach(() => {
    location.hash = "#/";
    resetForTests();
  });

  it("an active guard blocks navigate and records the target", () => {
    const onBlocked = vi.fn();
    setNavGuard(() => true, onBlocked);
    navigate("setup");
    expect(location.hash).toBe("#/"); // not changed
    expect(blockedRoute()).toBe("setup");
    expect(onBlocked).toHaveBeenCalledWith("setup");
  });

  it("navigate proceeds when no guard is set", () => {
    setNavGuard(null);
    navigate("presets");
    expect(location.hash).toBe("#/presets");
    expect(blockedRoute()).toBeNull();
  });

  it("navigating to the current route is never blocked", () => {
    setNavGuard(() => true, vi.fn());
    navigate(""); // already on the dashboard
    expect(location.hash).toBe("#/");
    expect(blockedRoute()).toBeNull();
  });

  it("a guard that returns false does not block", () => {
    setNavGuard(() => false, vi.fn());
    navigate("setup");
    expect(location.hash).toBe("#/setup");
  });

  it("proceed resumes a blocked navigation and clears the target", () => {
    setNavGuard(() => true, vi.fn());
    navigate("setup"); // blocked
    expect(location.hash).toBe("#/");
    proceed("setup");
    expect(location.hash).toBe("#/setup");
    expect(blockedRoute()).toBeNull();
  });
});
