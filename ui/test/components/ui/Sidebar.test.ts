import { fireEvent, render, screen } from "@testing-library/svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import SidebarHarness from "./SidebarHarness.svelte";
import type { Status } from "../../../src/lib/types";
import { authState } from "../../../src/lib/store/auth.svelte";

/// Stub `fetch` so `/api/status` resolves to the given (partial) body.
function mockApi(status: Partial<Status>) {
  return vi.fn((url: string) => {
    const body = url.includes("/api/status") ? status : {};
    return Promise.resolve({ ok: true, json: () => Promise.resolve(body) } as unknown as Response);
  });
}

beforeEach(() => {
  localStorage.clear();
  // Sidebar polls /api on mount; stub fetch so the unit test is hermetic.
  vi.stubGlobal("fetch", vi.fn(() => Promise.reject(new Error("no network in unit test"))));
});

afterEach(() => {
  vi.unstubAllGlobals();
  authState.enabled = false;
  authState.unlocked = false;
});

describe("Sidebar", () => {
  it("renders nav items with stable test ids", () => {
    render(SidebarHarness);
    expect(screen.getByTestId("nav-dashboard")).toBeVisible();
    expect(screen.getByTestId("nav-presets")).toBeVisible();
  });

  it("persists collapse state to localStorage", async () => {
    render(SidebarHarness);
    await fireEvent.click(screen.getByTestId("sidebar-toggle"));
    expect(localStorage.getItem("amd-gpu-tuned:sidebar")).toBe("0");
  });

  it("shows the no-sudo banner when the daemon is unprivileged", async () => {
    vi.stubGlobal(
      "fetch",
      mockApi({ privileged: false, version: "1.2.3" }),
    );
    render(SidebarHarness);
    expect(await screen.findByTestId("no-sudo-banner")).toBeVisible();
  });

  it("hides the no-sudo banner when the daemon is privileged", async () => {
    vi.stubGlobal(
      "fetch",
      mockApi({ privileged: true, version: "1.2.3" }),
    );
    render(SidebarHarness);
    // Wait for status to load (version renders), then assert no banner.
    expect(await screen.findByText(/v1\.2\.3/)).toBeVisible();
    expect(screen.queryByTestId("no-sudo-banner")).toBeNull();
  });

  it("does not render the baseline or watchdog footer lines", async () => {
    vi.stubGlobal(
      "fetch",
      mockApi({ privileged: true, version: "1.2.3" }),
    );
    render(SidebarHarness);
    // Wait for status to load so the footer is fully rendered.
    await screen.findByText(/v1\.2\.3/);
    expect(screen.queryByText("watchdog")).toBeNull();
    expect(screen.queryByText(/baseline/i)).toBeNull();
  });

  it("shows the locked indicator when auth is enabled", async () => {
    vi.stubGlobal(
      "fetch",
      mockApi({ privileged: true, version: "1.2.3", auth_enabled: true }),
    );
    render(SidebarHarness);
    expect(await screen.findByTestId("lock-indicator")).toHaveTextContent(
      "locked",
    );
  });

  it("shows no lock indicator when auth is disabled", async () => {
    vi.stubGlobal(
      "fetch",
      mockApi({ privileged: true, version: "1.2.3", auth_enabled: false }),
    );
    render(SidebarHarness);
    await screen.findByText(/v1\.2\.3/);
    expect(screen.queryByTestId("lock-indicator")).toBeNull();
  });

  it("clicking the unlocked indicator locks via the confirm dialog", async () => {
    authState.unlocked = true;
    const fetchMock = vi.fn((url: string) => {
      if (url.includes("/api/status")) {
        return Promise.resolve({
          ok: true,
          json: () =>
            Promise.resolve({
              privileged: true,
              version: "1.2.3",
              auth_enabled: true,
            }),
        } as unknown as Response);
      }
      if (url.includes("/api/auth/lock")) {
        return Promise.resolve({
          ok: true,
          json: () => Promise.resolve({ locked: true }),
        } as unknown as Response);
      }
      return Promise.reject(new Error("no network in unit test"));
    });
    vi.stubGlobal("fetch", fetchMock);
    render(SidebarHarness);
    const indicator = await screen.findByTestId("lock-indicator");
    expect(indicator).toHaveTextContent("unlocked");
    await fireEvent.click(indicator);
    await fireEvent.click(await screen.findByRole("button", { name: "lock now" }));
    // lockNow() resolves two microtask hops after the click; poll, don't assert bare.
    await vi.waitFor(() => expect(authState.unlocked).toBe(false));
    expect(
      fetchMock.mock.calls.some((c) => String(c[0]).includes("/api/auth/lock")),
    ).toBe(true);
  });
});
