import { defineConfig } from "astro/config";
import react from "@astrojs/react";
import { fileURLToPath } from "node:url";
import { release } from "./src/release.ts";

const here = (path) => fileURLToPath(new URL(path, import.meta.url));

export default defineConfig({
  site: process.env.SITE_URL || undefined,
  server: { port: 4321 },
  devToolbar: { enabled: false },
  integrations: [react()],
  vite: {
    define: { __MESA_VERSION__: JSON.stringify(release.version) },
    server: { fs: { allow: [".."] } },
    plugins: [
      {
        name: "mesa-demo-logo",
        enforce: "pre",
        async resolveId(source, importer, options) {
          if (!importer || !/\/app\/src\//.test(importer) || !/(^|\/)logo(\.ts)?$/.test(source)) return null;
          const resolved = await this.resolve(source, importer, { ...options, skipSelf: true });
          return resolved?.id.endsWith("/app/src/logo.ts") ? here("./src/demo/logo.ts") : null;
        },
      },
    ],
    resolve: {
      alias: {
        "@tauri-apps/api/core": here("./src/demo/tauri/core.ts"),
        "@tauri-apps/api/event": here("./src/demo/tauri/event.ts"),
        "@tauri-apps/api/window": here("./src/demo/tauri/window.ts"),
        "@tauri-apps/api/webview": here("./src/demo/tauri/webview.ts"),
        "@tauri-apps/api/app": here("./src/demo/tauri/app.ts"),
        "@tauri-apps/plugin-dialog": here("./src/demo/tauri/dialog.ts"),
      },
      dedupe: ["react", "react-dom", "lucide-react"],
    },
  },
});
