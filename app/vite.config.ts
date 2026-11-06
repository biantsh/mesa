/// <reference types="vitest/config" />
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { hoverOnlyWhereThePointerIsKnown } from "./build/hoverGate";

export default defineConfig({
  plugins: [react()],
  css: { postcss: { plugins: [hoverOnlyWhereThePointerIsKnown] } },
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/target/**", "**/src-tauri/runtime/**", "**/src-tauri/gen/**"] },
  },
  test: { include: ["src/**/*.test.ts", "build/**/*.test.ts"] },
});
