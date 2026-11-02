import { render } from "@testing-library/svelte";
import { describe, expect, it } from "vitest";
import { Cpu } from "@lucide/svelte";
import Icon from "../../../src/lib/components/ui/Icon.svelte";

describe("Icon", () => {
  it("renders an svg at the requested size", () => {
    const { container } = render(Icon, { props: { icon: Cpu, size: 24 } });
    const svg = container.querySelector("svg");
    expect(svg).not.toBeNull();
    expect(svg!.getAttribute("width")).toBe("24");
  });
});
