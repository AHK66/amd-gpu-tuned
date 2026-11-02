import { fireEvent, render, screen } from "@testing-library/svelte";
import { describe, expect, it, vi } from "vitest";
import ModalHarness from "./ModalHarness.svelte";
import { openModal, closeModal } from "../../../src/lib/store/modal.svelte";

describe("Modal", () => {
  it("is hidden until openModal, shows a dialog, closes on Escape", async () => {
    const { container } = render(ModalHarness);
    expect(container.querySelector('[role="dialog"]')).toBeNull();

    openModal("m1");
    await vi.waitFor(() => expect(container.querySelector('[role="dialog"]')).not.toBeNull());
    expect(screen.getByTestId("modal-body-btn")).toBeVisible();

    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    await vi.waitFor(() => expect(container.querySelector('[role="dialog"]')).toBeNull());
    closeModal();
  });
});
