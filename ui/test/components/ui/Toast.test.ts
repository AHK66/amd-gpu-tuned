import { render, screen } from "@testing-library/svelte";
import { describe, expect, it, vi } from "vitest";
import ToastHarness from "./ToastHarness.svelte";
import { toast, toasts, _resetToastsForTests } from "../../../src/lib/store/toast.svelte";

describe("Toast", () => {
  it("shows a success message then auto-dismisses", async () => {
    vi.useFakeTimers();
    _resetToastsForTests();
    render(ToastHarness);
    toast.success("saved");
    // Module-store updates made outside a component handler flush on a
    // microtask — await, as in the Modal test, instead of asserting
    // synchronously.
    await vi.waitFor(() => expect(screen.getByText("saved")).toBeVisible());
    vi.advanceTimersByTime(4500);
    await vi.waitFor(() => expect(screen.queryByText("saved")).toBeNull());
    vi.useRealTimers();
  });

  it("dedupes an identical toast that is already visible", async () => {
    vi.useFakeTimers();
    _resetToastsForTests();
    render(ToastHarness);
    toast.error("daemon down");
    await vi.waitFor(() => expect(toasts).toHaveLength(1));
    toast.error("daemon down");
    await vi.waitFor(() => expect(toasts).toHaveLength(1));
    expect(screen.getAllByText("daemon down")).toHaveLength(1);
    // The duplicate push refreshed the auto-dismiss timer: the surviving
    // toast outlives 4s after the first push.
    vi.advanceTimersByTime(4000);
    toast.error("daemon down");
    await vi.waitFor(() => expect(toasts).toHaveLength(1));
    vi.advanceTimersByTime(4500);
    await vi.waitFor(() => expect(toasts).toHaveLength(0));
    // A different tone with the same message is not deduped.
    toast.error("other");
    toast.info("other");
    await vi.waitFor(() => expect(toasts).toHaveLength(2));
    vi.useRealTimers();
  });
});
