/// <reference types="vitest/config" />
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

const hoverOnlyWhereThePointerIsKnown = {
  postcssPlugin: "hover-only-where-the-pointer-is-known",
  Rule(rule: { selector: string }) {
    if (!rule.selector.includes(":hover") || rule.selector.includes("data-pointer-unknown") || rule.selector.includes("::-webkit-scrollbar")) return;
    rule.selector = rule.selector.replace(/:hover(?![\w-])/g, ":hover:where(:root:not([data-pointer-unknown]) *)");
  },
};

export default defineConfig({
  plugins: [react()],
  css: { postcss: { plugins: [hoverOnlyWhereThePointerIsKnown] } },
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/target/**", "**/src-tauri/runtime/**", "**/src-tauri/gen/**"] },
  },
  test: { include: ["src/**/*.test.ts"] },
});
