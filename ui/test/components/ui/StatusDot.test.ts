import { render } from "@testing-library/svelte";
import { describe, expect, it } from "vitest";
import StatusDot from "../../../src/lib/components/ui/StatusDot.svelte";

describe("StatusDot", () => {
  it("renders a dot with the tone color", () => {
    const { container } = render(StatusDot, { props: { color: "success" } });
    expect(container.querySelector(".bg-success")).not.toBeNull();
  });
});
