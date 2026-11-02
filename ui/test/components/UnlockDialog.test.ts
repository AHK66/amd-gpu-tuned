import { fireEvent, render, screen } from "@testing-library/svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import UnlockDialog from "../../src/lib/components/UnlockDialog.svelte";
import { openModal, closeModal } from "../../src/lib/store/modal.svelte";
import { authState } from "../../src/lib/store/auth.svelte";
import { clearToken } from "../../src/lib/api";

function json(status: number, body: unknown) {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json" },
  });
}

beforeEach(() => {
  closeModal();
  clearToken();
  authState.unlocked = false;
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("UnlockDialog", () => {
  it("renders nothing while the unlock modal is closed", () => {
    render(UnlockDialog);
    expect(screen.queryByTestId("unlock-password")).toBeNull();
  });

  it("shows an inline error and stays open on a wrong password", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async (_url?: unknown, _init?: RequestInit) =>
        json(401, { error: "wrong password" }),
      ),
    );
    render(UnlockDialog);
    openModal("unlock");
    // openModal() flushes on a microtask — find, don't get (see Modal.test.ts).
    const input = (await screen.findByTestId("unlock-password")) as HTMLInputElement;
    input.value = "nope";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    // The submit button is disabled until the binding's DOM flush lands.
    const btn = screen.getByRole("button", { name: "unlock" });
    await vi.waitFor(() => expect(btn).not.toBeDisabled());
    await fireEvent.click(btn);
    expect(await screen.findByTestId("unlock-error")).toHaveTextContent(
      "wrong password",
    );
    expect(screen.queryByTestId("unlock-password")).not.toBeNull();
  });

  it("closes on success and flips unlocked", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async (_url?: unknown, _init?: RequestInit) =>
        json(200, { token: "t", ttl_s: 1800 }),
      ),
    );
    render(UnlockDialog);
    openModal("unlock");
    const input = (await screen.findByTestId("unlock-password")) as HTMLInputElement;
    input.value = "right";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    // The submit button is disabled until the binding's DOM flush lands.
    const btn = screen.getByRole("button", { name: "unlock" });
    await vi.waitFor(() => expect(btn).not.toBeDisabled());
    await fireEvent.click(btn);
    await vi.waitFor(() =>
      expect(screen.queryByTestId("unlock-password")).toBeNull(),
    );
    expect(authState.unlocked).toBe(true);
  });
});
