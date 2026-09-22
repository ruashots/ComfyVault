/**
 * Which engine the interface talks to.
 *
 * Inside the Tauri window it is the real one, built from the commands
 * comfyvault-core publishes in docs/IPC-CONTRACT.md. In a browser, during
 * development, it is the fixture engine. Both implement the same interface, so
 * no screen knows the difference.
 */

import type { Engine } from "~/ipc/contract";

/** True inside the desktop window, false in a browser tab. */
export function isDesktop(): boolean {
  return (
    typeof window !== "undefined" &&
    ("__TAURI_INTERNALS__" in window || "__TAURI__" in window)
  );
}

export async function createEngine(): Promise<Engine> {
  if (isDesktop()) {
    const { createTauriEngine } = await import("~/ipc/tauri");
    return createTauriEngine();
  }
  const { FixtureEngine } = await import("~/ipc/fixture/engine");
  // ?first-run opens the browser build with nothing registered.
  const empty = new URLSearchParams(window.location.search).has("first-run");
  const engine = new FixtureEngine({ empty });
  // The mock carried a control bar outside the window for setting the machine's
  // condition. This is the same thing for the running app: it sets Developer
  // Mode and whether ComfyUI is running, which only Windows can do for real.
  // It lives on this path only, so it cannot reach the desktop window.
  (window as unknown as { comfyVaultDev?: unknown }).comfyVaultDev = {
    developerMode: (on: boolean) => engine.devSetDeveloperMode(on),
    comfyRunning: (on: boolean) => engine.devSetComfyRunning(on),
    reset: (empty = false) => engine.devReset(empty),
  };
  return engine;
}
