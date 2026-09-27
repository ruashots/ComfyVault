import { screen } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";

import { App } from "~/App";
import { FixtureEngine } from "~/ipc/fixture/engine";
import { renderWithApp, waitFor, type Harness } from "~/test/render";

/**
 * The person's rule for the Library: "in library we don't manage comfy
 * filesystems, we manage the comfyvault". It lists what is in the vault, and
 * before the first run that is nothing.
 */

let harness: Harness | null = null;

afterEach(() => {
  harness?.unmount();
  harness = null;
});

const text = () => (document.body.textContent ?? "").replace(/\s+/g, " ");

async function openLibrary(engine: FixtureEngine) {
  harness = await renderWithApp(() => <App />, { engine });
  harness.app.actions.go("library");
  await waitFor(() => harness!.app.ready());
  return harness;
}

/** The sample vault before any run, with the three files it holds deleted. */
async function emptyVault(): Promise<FixtureEngine> {
  const engine = new FixtureEngine();
  for (const f of await engine.listOrphans()) await engine.deleteVaultFile(f.sha256, f.sha256);
  return engine;
}

describe("the Library before the first run", () => {
  it("lists nothing, though the scan found models in the installs", async () => {
    const engine = await emptyVault();
    const scan = (await engine.getLastScan())!;
    expect(scan.totals.uniqueContents).toBeGreaterThan(0);
    const { app } = await openLibrary(engine);
    expect(app.library()).toEqual([]);
    expect(app.libraryTotal()).toBe(0);
    expect(text()).toContain("The vault is empty");
    expect(text()).toContain(
      "The Library lists the models in the vault, and nothing is in it yet. Consolidate puts the models your installs share into the vault, and Download adds one from Hugging Face or Civitai.",
    );
    expect(text()).not.toContain("counted once each");
  });

  it("lists only the files the vault already holds, not the models in the installs", async () => {
    const engine = new FixtureEngine();
    const held = (await engine.listVaultFiles({ offset: 0, limit: 1000 })).files;
    expect(held.length).toBeGreaterThan(0);
    const { app } = await openLibrary(engine);
    await waitFor(() => app.library().length > 0);
    expect(new Set(app.library().map((r) => r.sha256))).toEqual(new Set(held.map((f) => f.sha256)));
  });

  it("says where to go next", async () => {
    const { app } = await openLibrary(await emptyVault());
    await userEvent.click(screen.getByRole("button", { name: "Go to Consolidate" }));
    expect(app.screen()).toBe("consolidate");
    app.actions.go("library");
    await waitFor(() => screen.queryByRole("button", { name: "Download a model" }) !== null);
    await userEvent.click(screen.getByRole("button", { name: "Download a model" }));
    expect(app.screen()).toBe("download");
  });
});

describe("the Library after a run", () => {
  it("lists only the models in the vault, and says so in its top line", async () => {
    const engine = new FixtureEngine({ manual: true });
    engine.devSetSymlinksSupported(true);
    engine.devSetComfyRunning(false);
    const scan = (await engine.getLastScan())!;
    const plan = await engine.buildPlan(scan.scanId);
    // Only three models go into the vault. The rest stay in the installs.
    await engine.startApply({ planId: plan.planId, groupIds: plan.groups.slice(0, 3).map((g) => g.groupId) });
    engine.devFinish();
    const vault = (await engine.listVaultFiles({ offset: 0, limit: 1000 })).files;

    const { app } = await openLibrary(engine);
    await waitFor(() => app.library().length > 0);
    expect(app.library().every((r) => r.inVault)).toBe(true);
    expect(new Set(app.library().map((r) => r.sha256))).toEqual(new Set(vault.map((f) => f.sha256)));
    expect(document.querySelector(".hdr .sub")!.textContent).toBe(`${vault.length} models in the vault`);
  });
});

describe("linking a vault model into an install from the Library", () => {
  it("asks for the install, then a folder ComfyUI reads, and makes the link there", async () => {
    const engine = new FixtureEngine();
    const orphan = (await engine.listOrphans())[0]!;
    const { app } = await openLibrary(engine);
    await waitFor(() => app.library().some((r) => r.sha256 === orphan.sha256));
    app.setLib({ selected: orphan.sha256, drawerOpen: true });
    await userEvent.click(await screen.findByRole("button", { name: /^Link into an/ }));
    const modal = () => document.querySelector('[aria-label="Where the link goes"]')!;
    await waitFor(() => document.querySelector('[aria-label="Where the link goes"]') !== null);
    expect(modal().textContent).toContain("Pick the install the link goes in. Then pick the folder inside it.");
    await userEvent.click(
      [...modal().querySelectorAll(".tnode")].find((b) => b.textContent?.startsWith("ComfyUI-Studio"))!,
    );
    await waitFor(() => modal().querySelector("h2")!.textContent === "Where the link goes in ComfyUI-Studio");
    await waitFor(() => modal().querySelectorAll(".tnode").length > 0);
    const usual = [...modal().querySelectorAll(".tnode")].find((n) => n.textContent?.includes("the usual place"))!;
    expect(usual.classList.contains("on")).toBe(true);
    await userEvent.click(screen.getByRole("button", { name: "Use this folder" }));
    await waitFor(() => document.querySelector('[aria-label="Where the link goes"]') === null);
    const links = await engine.listLinks({ sha256: orphan.sha256 });
    expect(links.map((l) => l.absPath)).toEqual([
      `C:\\ComfyUI-Studio\\models\\${orphan.category}\\${orphan.canonicalName}`,
    ]);
    expect(app.toast()!.message).toBe(`Linked ${orphan.canonicalName} in ComfyUI-Studio.`);
  });
});
