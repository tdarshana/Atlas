import { defineConfig } from "vite";
import { sveltekit } from "@sveltejs/kit/vite";

// `process` is typed now that @types/node is a dev dependency.
const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig(async () => ({
  plugins: [sveltekit()],

  // Component tests opt into jsdom with a `@vitest-environment` docblock; everything else
  // runs in node. The browser condition makes Svelte resolve its client build under vitest.
  resolve: process.env.VITEST ? { conditions: ["browser"] } : {},
  test: {
    include: ["src/**/*.{test,spec}.{js,ts}"],
    css: true,
  },

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
}));
