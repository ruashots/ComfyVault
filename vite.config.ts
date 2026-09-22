import { fileURLToPath } from "node:url";
import { defineConfig } from "vite";
import solid from "vite-plugin-solid";

// Tauri reads the built frontend from dist/ and, in development, from devUrl.
// Port 1420 is the port src-tauri/tauri.conf.json points devUrl at.
export default defineConfig({
  plugins: [solid()],
  clearScreen: false,
  resolve: {
    alias: { "~": fileURLToPath(new URL("./src", import.meta.url)) },
  },
  server: {
    port: 1420,
    strictPort: true,
  },
  // Tauri ships a fixed WebView2 runtime on Windows, so there is no need to
  // transpile down for old browsers.
  build: {
    target: "chrome105",
    sourcemap: true,
  },
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: ["./src/test/setup.ts"],
    // solid-js must not be externalised or the test renderer and the component
    // under test end up with two different reactive runtimes.
    deps: { optimizer: { web: { include: ["solid-js"] } } },
  },
});
