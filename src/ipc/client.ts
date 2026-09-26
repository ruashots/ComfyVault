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
  // Only Windows can really turn Developer Mode on or close ComfyUI, so the
  // browser build sets both from the console instead. It lives on this path
  // only, so it cannot reach the desktop window.
  (window as unknown as { comfyVaultDev?: unknown }).comfyVaultDev = {
    symlinks: (on: boolean) => engine.devSetSymlinksSupported(on),
    comfyRunning: (on: boolean) => engine.devSetComfyRunning(on),
    reset: (empty = false) => engine.devReset(empty),
    breakLinks: (count = 1) => engine.devBreakLinks(count),
    // The biggest model with two links or more loses this many of them, as a
    // delete cut off by the power going would leave it.
    stopDelete: async (linksGone = 1) => {
      const { files } = await engine.listVaultFiles({ offset: 0, limit: 1000 });
      const model = [...files].sort((a, b) => b.sizeBytes - a.sizeBytes).find((f) => f.linkCount >= 2);
      return model ? engine.devStopDelete(model.sha256, linksGone) : [];
    },
    forgetVersions: () => engine.devForgetVersions(),
    noWorkflows: () => engine.devSetWorkflowsOnDisk(0),
    runningIn: (...installIds: string[]) => engine.devSetRunningInstalls(installIds),
    processFacts: (facts: Parameters<typeof engine.devSetProcessFacts>[0]) =>
      engine.devSetProcessFacts(facts),
    taskManagerStarts: (starts: boolean) => engine.devSetTaskManagerStarts(starts),
  };
  return engine;
}
