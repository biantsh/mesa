import { describe, expect, it } from "vitest";
import { gateHover } from "./hoverGate";

const GATE = ":where(:root:not([data-pointer-unknown]) *)";

describe("gateHover", () => {
  it("gates :hover anywhere in the selector", () => {
    expect(gateHover(".button:hover")).toBe(`.button:hover${GATE}`);
    expect(gateHover(".row:hover .remove")).toBe(`.row:hover${GATE} .remove`);
    expect(gateHover(".primary:hover:not(:disabled)")).toBe(`.primary:hover${GATE}:not(:disabled)`);
    expect(gateHover(".raised:hover::before")).toBe(`.raised:hover${GATE}::before`);
    expect(gateHover(".collapsed:has(button.toggle:hover)")).toBe(`.collapsed:has(button.toggle:hover${GATE})`);
    expect(gateHover(':global([data-marquee="on"]) .stoppable:hover .status')).toBe(
      `:global([data-marquee="on"]) .stoppable:hover${GATE} .status`,
    );
  });

  it("is idempotent", () => {
    const once = gateHover(".row:hover .remove, .row:hover .busy");
    expect(gateHover(once)).toBe(once);
  });
});
