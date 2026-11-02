import { render, screen } from "@testing-library/svelte";
import { describe, expect, it } from "vitest";
import TableHarness from "./TableHarness.svelte";

describe("Table", () => {
  it("renders header cells and rows", () => {
    render(TableHarness);
    expect(screen.getByText("name")).toBeVisible();
    expect(screen.getByText("updated")).toBeVisible();
    expect(screen.getByText("e2e")).toBeVisible();
  });
});
