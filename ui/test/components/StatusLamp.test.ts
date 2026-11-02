import { render, screen } from "@testing-library/svelte";
import { describe, expect, it } from "vitest";
import { Clock } from "@lucide/svelte";
import StatusLamp from "../../src/lib/components/ui/StatusLamp.svelte";

describe("StatusLamp", () => {
  it("renders its testid and exposes the tooltip text in the DOM", () => {
    render(StatusLamp, {
      props: { icon: Clock, tone: "warn", text: "No fresh telemetry (link stale)", testid: "lamp-stale" },
    });
    expect(screen.getByTestId("lamp-stale")).toBeVisible();
    expect(screen.getByText("No fresh telemetry (link stale)")).toHaveTextContent(
      "No fresh telemetry (link stale)",
    );
  });

  it("applies the tone color classes (danger)", () => {
    render(StatusLamp, { props: { icon: Clock, tone: "danger", text: "t", testid: "lamp-d" } });
    expect(screen.getByTestId("lamp-d").className).toContain("text-danger");
  });

  it("defaults to the warn tone", () => {
    render(StatusLamp, { props: { icon: Clock, text: "t", testid: "lamp-w" } });
    expect(screen.getByTestId("lamp-w").className).toContain("text-warn");
  });
});
