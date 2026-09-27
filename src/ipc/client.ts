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
    // The first two-names card: its other name is taken in the install that
    // uses the picked one, and a second folder there links the other name.
    nameTaken: async () => {
      const [group] = await engine.listNameGroups();
      if (!group) return;
      const [picked, other] = group.names;
      const plan = await engine.planUnifyName(group.sha256, picked!.name);
      const moving = plan.links.find((s) => s.action === "rename");
      if (!moving) return;
      await engine.createLink({
        installId: plan.links.find((s) => s.action === "keep")!.installId,
        sha256: group.sha256,
        relativeDir: "models\\clip",
        linkName: other!.name,
      });
      engine.devTakePath(moving.newAbsPath!);
    },
    // The downloader. Each switch makes one state of the Download screen.
    download: {
      hold: (on = true) => engine.downloads.devHold(on),
      step: (n = 1) => {
        for (let i = 0; i < n; i++) engine.downloads.devStep();
      },
      finish: () => engine.downloads.devFinishDownloads(),
      dropConnection: () => engine.downloads.devDropConnection(),
      refuse: (message: string) => engine.downloads.devRefuseMidway(message),
      corruptNext: () => engine.downloads.devCorruptNext(),
      cutOff: () => engine.downloads.devCutOff(),
      freeGB: (gb: number) => engine.downloads.devSetFreeBytes(gb * 1024 ** 3),
      readDelay: (ms: number) => engine.downloads.devSetReadDelay(ms),
      slowStop: (on = true) => engine.downloads.devSlowStop(on),
      letGo: () => engine.downloads.devLetGo(),
      placeFile: (installId: string, category: string, name: string) =>
        engine.downloads.devPlaceFile(installId, category, name),
      revokeToken: (service: "huggingface" | "civitai") => engine.downloads.devRevokeToken(service),
      acceptTerms: (owner: string, repo: string) => engine.downloads.devAcceptTerms(owner, repo),
    },
  };
  return engine;
}
