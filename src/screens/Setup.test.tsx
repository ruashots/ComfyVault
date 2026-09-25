import { screen } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { createEffect, createRoot } from "solid-js";
import { afterEach, describe, expect, it, vi } from "vitest";

import { App } from "~/App";
import { FixtureEngine } from "~/ipc/fixture/engine";
import { renderWithApp, waitFor, type Harness } from "~/test/render";

let harness: Harness | null = null;

afterEach(() => {
  harness?.unmount();
  harness = null;
});

/** A vault chosen and these installs registered, and nothing scanned. */
async function setup(
  roots: string[] = [],
  prepare?: (engine: FixtureEngine) => void,
): Promise<Harness> {
  const engine = new FixtureEngine({ empty: true, manual: true });
  await engine.selectVault("C:\\ComfyVault");
  for (const root of roots) await engine.registerInstall(root);
  prepare?.(engine);
  harness = await renderWithApp(() => <App />, { engine });
  return harness;
}

const text = () => document.body.textContent ?? "";
const button = (name: string | RegExp) => screen.getByRole("button", { name });
const rows = () =>
  [...document.querySelectorAll(".sreg .nm")].map((el) => el.textContent);
const verdict = () => document.querySelector(".verdict:not(.wait)");

/** Open a folder in the picker tree, by its row name. */
async function open(name: string): Promise<void> {
  await userEvent.click(button(`Open ${name}`));
  const inside = name.endsWith("\\") ? name : `${name}\\`;
  await waitFor(() =>
    [...document.querySelectorAll(".tnode")].some((el) =>
      el.getAttribute("data-path")!.startsWith(inside),
    ),
  );
}

async function pick(path: string): Promise<void> {
  const row = document.querySelector(`.tnode[data-path="${CSS.escape(path)}"]`)!;
  await userEvent.click(row as HTMLElement);
  await waitFor(() => verdict() !== null);
}

async function openPicker(): Promise<void> {
  await userEvent.click(button(/Choose an install folder|Add another/));
  await waitFor(() => screen.queryAllByRole("dialog").length > 0);
}

describe("setup stays open until the first scan", () => {
  it("keeps the list after the first install, so the next one goes in the same place", async () => {
    const { app } = await setup();
    await openPicker();
    await open("C:\\");
    await pick("C:\\ComfyUI-Studio");
    await userEvent.click(button("Add this install"));
    await waitFor(() => app.installs().length === 1);

    expect(app.setupDone()).toBe(false);
    expect(rows()).toEqual(["ComfyUI-Studio"]);
    expect(text()).toContain("1 registered");
    expect(text()).toContain("both set · scan when every install is in the list");
    expect(button("Scan 1 install")).toBeEnabled();
  });

  it("lists each install with its label as it is, its path and its drive, and no size", async () => {
    await setup(["C:\\ComfyUI-Studio", "D:\\ComfyUI-Backup"]);
    const first = document.querySelectorAll(".sreg")[0]!;
    expect(first.querySelector(".nm")!.textContent).toBe("ComfyUI-Studio");
    expect(first.querySelector(".pp")!.getAttribute("title")).toBe("C:\\ComfyUI-Studio");
    expect(first.textContent).toContain("drive C:");
    expect(document.querySelectorAll(".sreg")[1]!.textContent).toContain("drive D:");
    // A size comes from a scan, and there has been none.
    expect(document.querySelector(".setup.col")!.textContent).not.toMatch(/\d+ (MB|GB|TB)/);
    expect(text()).toContain("Add every install before you scan.");
    expect(button("Scan 2 installs")).toBeEnabled();
  });

  it("marks the extra model paths file when an install has one", async () => {
    await setup(["C:\\ComfyUI-Portable"]);
    expect(document.querySelector(".sreg")!.textContent).toContain(
      "+ extra_model_paths.yaml",
    );
  });

  it("comes back to the same list when the app opens again before a scan", async () => {
    const { engine, unmount } = await setup(["C:\\ComfyUI-Studio", "C:\\ComfyUI-Sandbox"]);
    unmount();
    harness = await renderWithApp(() => <App />, { engine });
    expect(harness.app.setupDone()).toBe(false);
    expect(rows()).toEqual(["ComfyUI-Studio", "ComfyUI-Sandbox"]);
  });

  it("offers no scan until both things are set, and says what is missing", async () => {
    const engine = new FixtureEngine({ empty: true });
    harness = await renderWithApp(() => <App />, { engine });
    expect(button("Scan")).toBeDisabled();
    expect(button("Scan").getAttribute("title")).toBe("Choose the vault folder first");
    harness.unmount();

    await setup();
    expect(button("Scan")).toBeDisabled();
    expect(button("Scan").getAttribute("title")).toBe("Add an install first");
    expect(text()).toContain("1 of 2 done");
    expect(text()).toContain(
      "Add every ComfyUI install on this computer. The folder picker stays open, so you can add one after another.",
    );
  });

  it("tells the other screens that nothing has been read, not that a step is missing", async () => {
    const { app } = await setup(["C:\\ComfyUI-Studio"]);
    app.actions.go("consolidate");
    await waitFor(() => document.querySelector(".empty") !== null);
    expect(text()).toContain("Your installs are registered and nothing has been read yet.");
  });
});

describe("the scan button is the way out of setup", () => {
  it("starts one scan and leaves setup when the scan reports", async () => {
    const { app, engine } = await setup(["C:\\ComfyUI-Studio", "C:\\ComfyUI-Sandbox"]);
    const start = vi.spyOn(engine, "startScan");
    await userEvent.click(button("Scan 2 installs"));
    await waitFor(() => app.appState()?.busy?.kind === "scan");

    // The engine has it and has not reported yet: the button cannot be
    // pressed a second time.
    expect(button("Scan 2 installs")).toBeDisabled();
    await userEvent.click(button("Scan 2 installs"));
    expect(start).toHaveBeenCalledTimes(1);

    engine.devAdvance();
    await waitFor(() => app.scanProgress() !== null);
    expect(app.setupDone()).toBe(true);
    expect(text()).not.toContain("Two things to set");
  });

  it("does not come back while the finished scan is read back", async () => {
    const { app, engine } = await setup(["C:\\ComfyUI-Studio"]);
    await userEvent.click(button("Scan 1 install"));
    await waitFor(() => app.appState()?.busy?.kind === "scan");
    engine.devAdvance();
    await waitFor(() => app.scanProgress() !== null);

    // Every value setup takes on the way, not only the last one.
    let setupSeen = false;
    const stop = createRoot((dispose) => {
      createEffect(() => {
        if (!app.setupDone()) setupSeen = true;
      });
      return dispose;
    });
    engine.devFinish();
    await waitFor(() => app.plan() !== null);
    stop();
    expect(setupSeen).toBe(false);
  });

  it("does not start a scan when an install is added", async () => {
    const { app, engine } = await setup();
    const start = vi.spyOn(engine, "startScan");
    await openPicker();
    await open("C:\\");
    await pick("C:\\ComfyUI-Studio");
    await userEvent.click(button("Add this install"));
    await waitFor(() => app.installs().length === 1);
    expect(start).not.toHaveBeenCalled();
  });
});

describe("removing an install from the list", () => {
  it("forgets it at once, touches nothing, and stays in setup", async () => {
    const { app, engine } = await setup(["C:\\ComfyUI-Studio", "C:\\ComfyUI-Sandbox"]);
    await userEvent.click(button("Remove C:\\ComfyUI-Studio from the list"));
    await waitFor(() => app.installs().length === 1);

    expect(screen.queryAllByRole("dialog")).toHaveLength(0);
    expect(rows()).toEqual(["ComfyUI-Sandbox"]);
    expect((await engine.listInstalls()).map((i) => i.root)).toEqual([
      "C:\\ComfyUI-Sandbox",
    ]);
    expect(text()).toContain(
      "Removed C:\\ComfyUI-Studio from the list · nothing in it was touched",
    );
    // Forgetting an install makes no scan record, so setup is still here.
    expect(await engine.getLastScan()).toBeNull();
    expect(app.setupDone()).toBe(false);
  });
});

describe("the picker stays open between installs", () => {
  it("says what it added, clears the pick, keeps the tree, and offers Done", async () => {
    const { app } = await setup();
    await openPicker();
    await open("C:\\");
    await pick("C:\\ComfyUI-Studio");
    await userEvent.click(button("Add this install"));
    await waitFor(() => app.installs().length === 1);

    expect(screen.getByRole("dialog")).toBeInTheDocument();
    expect(document.querySelector(".added")!.textContent).toBe(
      "Added C:\\ComfyUI-Studio · 1 registered. Pick the next install, or press Done.",
    );
    expect(document.querySelector(".tnode.on")).toBeNull();
    expect(document.querySelector(".picked")).toBeNull();
    // The drive the person opened is still open.
    expect(document.querySelector('.tnode[data-path="C:\\\\ComfyUI-Sandbox"]')).not.toBeNull();
    expect(button("Done")).toHaveClass("pri");
    expect(screen.queryByRole("button", { name: "Cancel" })).toBeNull();
    // No toast repeats what the line says.
    expect(document.querySelector(".toast")).toBeNull();
  });

  it("marks the one just added in the list behind, until the next scan starts", async () => {
    const { app, engine } = await setup(["C:\\ComfyUI-Sandbox"]);
    await openPicker();
    await open("C:\\");
    await pick("C:\\ComfyUI-Studio");
    await userEvent.click(button("Add this install"));
    await waitFor(() => app.installs().length === 2);

    const fresh = [...document.querySelectorAll(".sreg.fresh .nm")].map((e) => e.textContent);
    expect(fresh).toEqual(["ComfyUI-Studio"]);

    await userEvent.click(button("Done"));
    await userEvent.click(button("Scan 2 installs"));
    await waitFor(() => app.appState()?.busy?.kind === "scan");
    engine.devAdvance();
    await waitFor(() => app.scanProgress() !== null);
    expect(app.freshInstalls()).toEqual([]);
  });

  it("ticks every registered install in the tree", async () => {
    await setup(["C:\\ComfyUI-Studio"]);
    await openPicker();
    await open("C:\\");
    const studio = document.querySelector('.tnode[data-path="C:\\\\ComfyUI-Studio"]')!;
    const sandbox = document.querySelector('.tnode[data-path="C:\\\\ComfyUI-Sandbox"]')!;
    expect(studio.classList.contains("reg")).toBe(true);
    expect(studio.querySelector(".treg-tag")!.textContent).toBe("registered");
    expect(sandbox.querySelector(".treg-tag")).toBeNull();
  });

  it("says a registered one is in the list already", async () => {
    await setup(["C:\\ComfyUI-Studio"]);
    await openPicker();
    await open("C:\\");
    await pick("C:\\ComfyUI-Studio");
    expect(verdict()!.querySelector("h4")!.textContent).toBe("Already registered");
    expect(verdict()!.textContent).toContain(
      "ComfyUI-Studio is in the list already. Nothing to add.",
    );
    expect(button("Add this install")).toBeDisabled();
  });
});

describe("a folder that holds several installs", () => {
  it("offers every one of them at once, with the copy that another drive costs", async () => {
    const { app } = await setup();
    await openPicker();
    await open("D:\\");
    await pick("D:\\AI");

    expect(verdict()!.querySelector("h4")!.textContent).toBe(
      "3 ComfyUI installs inside this folder",
    );
    const body = verdict()!.textContent ?? "";
    expect(body).toContain("D:\\AI is not an install itself. ComfyVault found these inside it:");
    expect([...verdict()!.querySelectorAll(".cands span")].map((e) => e.textContent)).toEqual([
      "D:\\AI\\ComfyUI-Flux",
      "D:\\AI\\ComfyUI-SDXL",
      "D:\\AI\\ComfyUI-Video",
    ]);
    expect(body).toContain("To add only one, pick it in the list above.");
    expect(body).toContain(
      "They are on D: and the vault is on C:. Their files are copied, not moved, so C: pays for them before D: gives anything back. ComfyVault checks the free space before it starts.",
    );

    await userEvent.click(button("Add these 3 installs"));
    await waitFor(() => app.installs().length === 3);
    expect(app.installs().map((i) => i.root)).toEqual([
      "D:\\AI\\ComfyUI-Flux",
      "D:\\AI\\ComfyUI-SDXL",
      "D:\\AI\\ComfyUI-Video",
    ]);
    expect(document.querySelector(".added")!.textContent).toBe(
      "Added 3 installs · 3 registered. Pick the next install, or press Done.",
    );
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    expect([...document.querySelectorAll(".sreg.fresh .nm")].map((e) => e.textContent)).toEqual([
      "ComfyUI-Flux",
      "ComfyUI-SDXL",
      "ComfyUI-Video",
    ]);
  });

  it("offers only the ones not in the list, and names the others", async () => {
    const { app } = await setup(["D:\\AI\\ComfyUI-Flux"]);
    await openPicker();
    await open("D:\\");
    await pick("D:\\AI");

    expect(verdict()!.querySelector("h4")!.textContent).toBe(
      "2 ComfyUI installs inside this folder",
    );
    expect(verdict()!.textContent).toContain("ComfyUI-Flux is already registered.");
    await userEvent.click(button("Add these 2 installs"));
    await waitFor(() => app.installs().length === 3);
    expect(app.installs().map((i) => i.root)).toContain("D:\\AI\\ComfyUI-Video");
  });

  it("adds nothing when every one is registered", async () => {
    await setup(["D:\\AI\\ComfyUI-Flux", "D:\\AI\\ComfyUI-SDXL", "D:\\AI\\ComfyUI-Video"]);
    await openPicker();
    await open("D:\\");
    await pick("D:\\AI");
    expect(verdict()!.querySelector("h4")!.textContent).toBe("Already registered");
    expect(verdict()!.textContent).toContain(
      "Every ComfyUI install inside D:\\AI is in the list already. Nothing to add.",
    );
    expect(button("Add this install")).toBeDisabled();
  });

  it("keeps what landed when the engine refuses one part way", async () => {
    const { app, engine } = await setup();
    const real = engine.registerInstall.bind(engine);
    vi.spyOn(engine, "registerInstall").mockImplementation(async (path, label) => {
      if (path === "D:\\AI\\ComfyUI-SDXL") {
        throw { code: "permissionDenied", message: "Windows refused to read that folder." };
      }
      return real(path, label);
    });
    await openPicker();
    await open("D:\\");
    await pick("D:\\AI");
    await userEvent.click(button("Add these 3 installs"));
    await waitFor(() => document.querySelector('.verdict.no[role="alert"]') !== null);

    expect(app.installs().map((i) => i.root)).toEqual(["D:\\AI\\ComfyUI-Flux"]);
    expect(rows()).toEqual(["ComfyUI-Flux"]);
    expect(document.querySelector('.verdict.no[role="alert"]')!.textContent).toContain(
      "Windows refused to read that folder.",
    );
    expect(screen.getByRole("dialog")).toBeInTheDocument();
  });
});

describe("a ComfyUI running during setup", () => {
  it("is named under its row, and is not a reason to stop", async () => {
    await setup(["C:\\ComfyUI-Studio"], (engine) => {
      engine.devSetRunningInstalls(["comfyui-studio"]);
      const d = new Date();
      engine.devSetProcessFacts({
        startedAt: new Date(d.getFullYear(), d.getMonth(), d.getDate() - 1, 18, 42).toISOString(),
      });
    });
    const line = document.querySelector(".srun")!;
    expect(line.textContent).toBe(
      "running now · python.exe, pid 18244, started yesterday at 18:42 · scanning works, Apply waits until it is closed",
    );
    expect(line.querySelector(".led.up")).not.toBeNull();
    expect(button("Scan 1 install")).toBeEnabled();
  });
});

describe("Home when the first scan was cancelled", () => {
  it("says nothing was read, why, and offers both ways on", async () => {
    const { app, engine } = await setup(["C:\\ComfyUI-Studio", "C:\\ComfyUI-Sandbox"]);
    await userEvent.click(button("Scan 2 installs"));
    await waitFor(() => app.appState()?.busy?.kind === "scan");
    engine.devAdvance();
    await waitFor(() => app.scanProgress() !== null);
    await engine.cancelScan(app.scanProgress()!.scanId);
    await waitFor(() => app.scan()?.cancelled === true && app.scanProgress() === null);

    // A cancelled scan is on record, so setup does not come back.
    expect(app.setupDone()).toBe(true);
    await waitFor(() => text().includes("nothing has been read yet"));
    const hero = document.querySelector(".hero")!.textContent ?? "";
    expect(hero).toContain("2 installs are registered and nothing has been read yet.");
    expect(hero).toContain("The last scan was cancelled before it finished.");
    expect(button("Add an install")).toBeEnabled();
    expect(button("Scan 2 installs")).toBeEnabled();

    // Its totals are zeros, which are not sizes.
    const inst = [...document.querySelectorAll(".inst")].map((r) => r.textContent ?? "");
    expect(inst).toHaveLength(2);
    for (const row of inst) {
      expect(row).toContain("not read yet");
      expect(row).not.toContain("0 MB");
    }
    expect(document.querySelector(".tiles")).toBeNull();
    expect(document.querySelector(".rail")!.textContent).toContain("nothing read yet");
    expect(document.querySelector(".rail")!.textContent).not.toContain("0 MB");
  });

  it("opens the install picker from Home", async () => {
    const { app, engine } = await setup(["C:\\ComfyUI-Studio"]);
    await userEvent.click(button("Scan 1 install"));
    await waitFor(() => app.appState()?.busy?.kind === "scan");
    engine.devAdvance();
    await waitFor(() => app.scanProgress() !== null);
    await engine.cancelScan(app.scanProgress()!.scanId);
    await waitFor(() => text().includes("The last scan was cancelled"));

    await userEvent.click(button("Add an install"));
    await waitFor(() => screen.queryAllByRole("dialog").length > 0);
    expect(screen.getByRole("dialog").getAttribute("aria-label")).toBe(
      "Choose a ComfyUI install folder",
    );
  });
});

describe("Settings before the first scan", () => {
  it("says an install has not been read rather than printing a size", async () => {
    const { app } = await setup(["C:\\ComfyUI-Studio"]);
    app.actions.go("settings");
    await waitFor(() => document.querySelector(".icard") !== null);
    const card = document.querySelector(".icard")!.textContent ?? "";
    expect(card).toContain("not read yet");
    expect(card).not.toContain("0 files");
    expect(card).not.toContain("0 MB");
  });
});
