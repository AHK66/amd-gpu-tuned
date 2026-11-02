import { fireEvent, render, screen } from "@testing-library/svelte";
import { beforeEach, describe, expect, it, vi } from "vitest";
import Setup from "../../src/routes/Setup.svelte";
import type { SetupCheck } from "../../src/lib/types";
import { toasts, _resetToastsForTests } from "../../src/lib/store/toast.svelte";
import { getDriftSetup, getRingMinutes, setDriftSetup, setRingMinutes } from "../../src/lib/api";
import { authState } from "../../src/lib/store/auth.svelte";

const CHECK: SetupCheck = {
  ready: true,
  live_mask: 0xfff7ffff,
  overdrive_bit: true,
  gpus: [
    {
      slot: "1002:7551-1458:242f-0000:0c:00.0",
      overdrive_table: true,
      pmfw_fan: true,
      hwmon: true,
      dpm_levels: true,
      thermal_target: false,
      power_profile: true,
    },
  ],
};

vi.mock("../../src/lib/api", () => ({
  getSetup: vi.fn(async () => CHECK),
  getRingMinutes: vi.fn(async () => ({ minutes: 10 })),
  setRingMinutes: vi.fn(async (m: number) => ({ minutes: m })),
  getDriftSetup: vi.fn(async () => ({ reapply: true, delay_s: 30 })),
  setDriftSetup: vi.fn(async (b: { reapply: boolean; delay_s: number }) => b),
}));

describe("Setup", () => {
  beforeEach(() => {
    _resetToastsForTests();
    vi.mocked(getRingMinutes).mockReset();
    vi.mocked(setRingMinutes).mockReset();
    vi.mocked(getRingMinutes).mockResolvedValue({ minutes: 10 });
    vi.mocked(setRingMinutes).mockImplementation(async (m: number) => ({ minutes: m }));
    vi.mocked(getDriftSetup).mockReset();
    vi.mocked(setDriftSetup).mockReset();
    vi.mocked(getDriftSetup).mockResolvedValue({ reapply: true, delay_s: 30 });
    vi.mocked(setDriftSetup).mockImplementation(async (b) => b);
    authState.enabled = false;
    authState.unlocked = false;
  });

  it("shows the telemetry history card prefilled from the server", async () => {
    render(Setup);
    await screen.findByText("Telemetry history");
    const input = screen.getByTestId("ring-minutes") as HTMLInputElement;
    expect(input.value).toBe("10");
  });

  it("applies the ring window on Enter and toasts success", async () => {
    render(Setup);
    await screen.findByText("Telemetry history");
    const input = screen.getByTestId("ring-minutes") as HTMLInputElement;
    input.value = "20";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    expect(setRingMinutes).toHaveBeenCalledWith(20);
    await vi.waitFor(() => expect(input.value).toBe("20"));
    await vi.waitFor(() =>
      expect(toasts.some((t) => t.msg === "History window set to 20 minutes")).toBe(true),
    );
  });

  it("reverts to the server value and toasts an error when the server rejects", async () => {
    vi.mocked(setRingMinutes).mockRejectedValueOnce(new Error("boom"));
    render(Setup);
    await screen.findByText("Telemetry history");
    const input = screen.getByTestId("ring-minutes") as HTMLInputElement;
    input.value = "20";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    await vi.waitFor(() => expect(input.value).toBe("10"));
    await vi.waitFor(() => expect(toasts.some((t) => t.tone === "error")).toBe(true));
  });

  it("applies the ring window on input after 1s of quiet", async () => {
    render(Setup);
    await screen.findByText("Telemetry history");
    const input = screen.getByTestId("ring-minutes") as HTMLInputElement;
    vi.useFakeTimers();
    input.value = "25";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    expect(setRingMinutes).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1000);
    expect(setRingMinutes).toHaveBeenCalledWith(25);
    vi.useRealTimers();
    await vi.waitFor(() => expect(input.value).toBe("25"));
    await vi.waitFor(() =>
      expect(toasts.some((t) => t.msg === "History window set to 25 minutes")).toBe(true),
    );
  });

  it("restarts the 1s debounce on every change and commits only the last value", async () => {
    render(Setup);
    await screen.findByText("Telemetry history");
    const input = screen.getByTestId("ring-minutes") as HTMLInputElement;
    vi.useFakeTimers();
    input.value = "25";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    vi.advanceTimersByTime(900);
    input.value = "30";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    vi.advanceTimersByTime(900);
    expect(setRingMinutes).not.toHaveBeenCalled();
    vi.advanceTimersByTime(200);
    expect(setRingMinutes).toHaveBeenCalledWith(30);
    expect(setRingMinutes).not.toHaveBeenCalledWith(25);
    vi.useRealTimers();
  });

  it("does not commit an invalid value on input (no API call, no toast)", async () => {
    render(Setup);
    await screen.findByText("Telemetry history");
    const input = screen.getByTestId("ring-minutes") as HTMLInputElement;
    input.value = "99";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    expect(setRingMinutes).not.toHaveBeenCalled();
    expect(toasts).toHaveLength(0);
    expect(input.value).toBe("99");
  });

  it("blocks an out-of-range value on Enter with a toast and no API call", async () => {
    render(Setup);
    await screen.findByText("Telemetry history");
    const input = screen.getByTestId("ring-minutes") as HTMLInputElement;
    input.value = "99";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    expect(setRingMinutes).not.toHaveBeenCalled();
    expect(input.value).toBe("99");
    await vi.waitFor(() =>
      expect(toasts.some((t) => t.msg === "enter a whole number between 1 and 60")).toBe(true),
    );
  });

  it("renders the four gates", async () => {
    render(Setup);
    await screen.findByText("overdrive table");
    expect(screen.getByText("pmfw fan")).toBeTruthy();
    expect(screen.getByText("hwmon")).toBeTruthy();
    expect(screen.getByText("dpm levels")).toBeTruthy();
  });

  it("does not render thermal target or power profile", async () => {
    render(Setup);
    await screen.findByText("overdrive table");
    expect(screen.queryByText(/thermal target/i)).toBeNull();
    expect(screen.queryByText(/power profile/i)).toBeNull();
  });

  it("does not render the re-run check button", async () => {
    render(Setup);
    await screen.findByText("overdrive table");
    expect(screen.queryByText(/re-run check/i)).toBeNull();
  });

  it("labels the PCI address", async () => {
    render(Setup);
    await screen.findByText(/PCI address/);
    expect(screen.getByText(/1002:7551-1458:242f-0000:0c:00\.0/)).toBeTruthy();
  });

  it("shows the PP feature count as primary text and the raw mask in a tooltip", async () => {
    render(Setup);
    const primary = await screen.findByText("PP features: 31 enabled");
    expect(primary.textContent).not.toContain("0xfff7ffff");
    const tips = screen.getAllByRole("tooltip");
    expect(tips.some((t) => (t.textContent ?? "").includes("0xfff7ffff"))).toBe(true);
  });

  it("shows the drift recovery card prefilled from the server", async () => {
    render(Setup);
    await screen.findByText("Drift recovery");
    const toggle = screen.getByTestId("drift-reapply-toggle") as HTMLInputElement;
    expect(toggle.checked).toBe(true);
    const input = screen.getByTestId("drift-delay") as HTMLInputElement;
    expect(input.value).toBe("30");
  });

  it("commits both fields on toggle change", async () => {
    render(Setup);
    await screen.findByText("Drift recovery");
    const toggle = screen.getByTestId("drift-reapply-toggle") as HTMLInputElement;
    await fireEvent.click(toggle);
    await vi.waitFor(() =>
      expect(setDriftSetup).toHaveBeenCalledWith({ reapply: false, delay_s: 30 }),
    );
  });

  it("commits the delay on Enter and toasts success", async () => {
    render(Setup);
    await screen.findByText("Drift recovery");
    const input = screen.getByTestId("drift-delay") as HTMLInputElement;
    input.value = "45";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    await vi.waitFor(() =>
      expect(setDriftSetup).toHaveBeenCalledWith({ reapply: true, delay_s: 45 }),
    );
    await vi.waitFor(() =>
      expect(toasts.some((t) => t.msg === "Restore delay set to 45 s")).toBe(true),
    );
  });

  it("commits the delay after 1s of quiet", async () => {
    render(Setup);
    await screen.findByText("Drift recovery");
    const input = screen.getByTestId("drift-delay") as HTMLInputElement;
    vi.useFakeTimers();
    input.value = "60";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    expect(setDriftSetup).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1000);
    expect(setDriftSetup).toHaveBeenCalledWith({ reapply: true, delay_s: 60 });
    vi.useRealTimers();
  });

  it("blocks an out-of-range delay on Enter with a toast", async () => {
    render(Setup);
    await screen.findByText("Drift recovery");
    const input = screen.getByTestId("drift-delay") as HTMLInputElement;
    input.value = "999";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    expect(setDriftSetup).not.toHaveBeenCalled();
    await vi.waitFor(() =>
      expect(toasts.some((t) => t.msg === "enter a whole number between 5 and 600")).toBe(true),
    );
  });

  it("dims and disables the delay input while the toggle is off", async () => {
    vi.mocked(getDriftSetup).mockResolvedValue({ reapply: false, delay_s: 30 });
    render(Setup);
    await screen.findByText("Drift recovery");
    const input = screen.getByTestId("drift-delay") as HTMLInputElement;
    await vi.waitFor(() => expect(input.disabled).toBe(true));
  });

  it("shows the no-password note when auth is disabled", async () => {
    render(Setup);
    expect(await screen.findByTestId("no-password-note")).toBeVisible();
  });

  it("hides the no-password note when auth is enabled", async () => {
    authState.enabled = true;
    render(Setup);
    await screen.findByText("Telemetry history");
    expect(screen.queryByTestId("no-password-note")).toBeNull();
  });
});
