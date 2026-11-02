import { render, screen } from "@testing-library/svelte";
import { describe, expect, it, vi } from "vitest";
import ConfirmDialog from "../../../src/lib/components/ui/ConfirmDialog.svelte";
import { openModal, closeModal } from "../../../src/lib/store/modal.svelte";

describe("ConfirmDialog", () => {
  it("defaults the confirm button to the primary variant", async () => {
    render(ConfirmDialog, {
      props: {
        id: "cd",
        title: "T",
        body: "B",
        confirmLabel: "Go",
        onConfirm: () => {},
      },
    });
    openModal("cd");
    await vi.waitFor(() =>
      expect(screen.getByRole("button", { name: "Go" })).toBeVisible(),
    );
    const btn = screen.getByRole("button", { name: "Go" });
    expect(btn.className).toContain("bg-text"); // primary
    closeModal();
  });

  it("forwards a danger variant to the confirm button", async () => {
    render(ConfirmDialog, {
      props: {
        id: "cd2",
        title: "T",
        body: "B",
        confirmLabel: "Delete",
        variant: "danger",
        onConfirm: () => {},
      },
    });
    openModal("cd2");
    await vi.waitFor(() =>
      expect(screen.getByRole("button", { name: "Delete" })).toBeVisible(),
    );
    const btn = screen.getByRole("button", { name: "Delete" });
    expect(btn.className).toContain("bg-danger"); // danger
    closeModal();
  });
});
