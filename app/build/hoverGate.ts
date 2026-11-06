const GATE = ":where(:root:not([data-pointer-unknown]) *)";

export function gateHover(selector: string): string {
  if (!selector.includes(":hover") || selector.includes("data-pointer-unknown") || selector.includes("::-webkit-scrollbar")) {
    return selector;
  }
  return selector.replace(/:hover(?![\w-])/g, `:hover${GATE}`);
}

export const hoverOnlyWhereThePointerIsKnown = {
  postcssPlugin: "hover-only-where-the-pointer-is-known",
  Rule(rule: { selector: string }) {
    rule.selector = gateHover(rule.selector);
  },
};
