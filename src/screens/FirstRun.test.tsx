import { screen } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";

import { App } from "~/App";
import { FixtureEngine } from "~/ipc/fixture/engine";
import { renderWithApp, waitFor, type Harness } from "~/test/render";

let harness: Harness | null = null;

afterEach(() => {
  harness?.unmount();
  harness = null;
});

/** What a person meets the first time they ever open this: no vault, nothing. */
async function firstRun(): Promise<Harness> {
  const engine = new FixtureEngine({ empty: true });
  harness = await renderWithApp(() => <App />, { engine });
  await waitFor(() => harness!.app.ready());
  return harness;
}

describe("the first time anyone opens this", () => {
  it("invites them to start, and does not call it a failure", async () => {
    const { app } = await firstRun();

    expect(app.failure()).toBeNull();
    const text = document.body.textContent ?? "";
    expect(text).toContain("Nothing registered yet");
    expect(
      screen.getByRole("button", { name: /Choose an install folder/ }),
    ).toBeInTheDocument();

    // None of the apology, and none of the button that cannot do anything.
    expect(text).not.toContain("could not read the vault");
    expect(text).not.toContain("answered with a problem");
    expect(screen.queryByRole("button", { name: "Try again" })).toBeNull();
    // And no engine sentence about vaults reaches them either.
    expect(text).not.toContain("No vault folder is open yet");
  });

  it("asks the engine for nothing it refuses to answer", async () => {
    const engine = new FixtureEngine({ empty: true });
    const refused: string[] = [];
    for (const name of [
      "listInstalls",
      "getLastScan",
      "getInterruptedApplies",
      "listApplies",
      "getRunningComfy",
      "getVaultInfo",
      "listVaultFiles",
      "listContents",
      "listNameGroups",
      "listOrphans",
      "checkVaultHealth",
      "getSettings",
    ] as const) {
      const original = engine[name].bind(engine) as (...args: never[]) => unknown;
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      (engine as any)[name] = (...args: never[]) => {
        refused.push(name);
        return original(...args);
      };
    }
    harness = await renderWithApp(() => <App />, { engine });
    await waitFor(() => harness!.app.ready());

    // Every one of these refuses before a vault is chosen. Calling any of them
    // is what turned a first run into a failure screen.
    expect(refused).toEqual([]);
    expect(harness.app.failure()).toBeNull();
  });

  it("says where the vault will go, so the default is not a surprise", async () => {
    await firstRun();
    const text = document.body.textContent ?? "";
    expect(text).toContain("The vault will be created at");
    expect(text).toContain("C:\\ComfyVault");
    expect(text).toContain("Change that in Settings before the first scan");
  });

  it("leaves nothing half-filled behind it", async () => {
    const { app } = await firstRun();
    expect(app.vault()).toBeNull();
    expect(app.installs()).toEqual([]);
    expect(app.plan()).toBeNull();
    expect(app.scan()).toBeNull();
    expect(app.library()).toEqual([]);
    expect(app.health()).toBeNull();
    expect(app.danglingLinks()).toEqual([]);
    expect(app.hasInstalls()).toBe(false);
  });
});

describe("Settings before a vault folder exists", () => {
  it("leaves no heading with nothing under it", async () => {
    const { app } = await firstRun();
    app.actions.go("settings");
    await waitFor(() => document.body.textContent?.includes("The vault folder") === true);

    const text = document.body.textContent ?? "";
    expect(text).toContain("The vault folder");
    // The section says what will happen, rather than standing empty.
    expect(text).toContain("not created yet");
    expect(text).toContain("C:\\ComfyVault");
    expect(
      screen.getByRole("button", { name: /Choose/ }),
    ).toBeInTheDocument();
  });

  it("does not claim the drive has no room on it", async () => {
    const { app } = await firstRun();
    app.actions.go("settings");
    await waitFor(() => document.body.textContent?.includes("Free space") === true);

    const text = document.body.textContent ?? "";
    // "0 MB free" is a claim about a drive nobody has named yet.
    expect(text).not.toContain("0 MB free");
    expect(text).toContain("not known until a vault folder is chosen");
  });

  it("says the same about every other setting it cannot know", async () => {
    const { app } = await firstRun();
    app.actions.go("settings");
    await waitFor(() => document.body.textContent?.includes("What a scan reads") === true);
    const text = document.body.textContent ?? "";
    expect(text).not.toContain("undefined");
    expect(text).not.toContain("NaN");
  });
});

describe("a vault with nothing registered in it yet", () => {
  it("shows the same invitation rather than an empty Home", async () => {
    const engine = new FixtureEngine({ empty: true });
    // The vault folder exists now. The person still has no installs.
    await engine.selectVault("C:\\ComfyVault");
    harness = await renderWithApp(() => <App />, { engine });
    await waitFor(() => harness!.app.ready());

    expect(harness.app.failure()).toBeNull();
    expect(harness.app.vault()).not.toBeNull();
    expect(document.body.textContent).toContain("Nothing registered yet");
    expect(
      screen.getByRole("button", { name: /Choose an install folder/ }),
    ).toBeInTheDocument();
  });
});

describe("a vault that goes away between two calls", () => {
  it("falls back to the invitation rather than an apology", async () => {
    const engine = new FixtureEngine();
    // The state says a vault is open, and the very next call disagrees. That
    // happens when the vault folder is on a drive that has just been pulled,
    // and it must not read as "this program is broken".
    engine.listInstalls = async () => {
      throw {
        code: "notInitialized",
        message: "No vault folder is open yet. Choose a vault folder to continue.",
      };
    };
    harness = await renderWithApp(() => <App />, { engine });
    await waitFor(() => harness!.app.ready());

    expect(harness.app.failure()).toBeNull();
    const text = document.body.textContent ?? "";
    expect(text).toContain("Nothing registered yet");
    expect(text).not.toContain("could not read the vault");
    expect(text).not.toContain("No vault folder is open yet");
  });
});

describe("one command that becomes conditional later", () => {
  it("costs that one answer, not the whole window", async () => {
    const engine = new FixtureEngine();
    // Any of these could grow a vault requirement the interface has not caught
    // up with. Losing the window over it is the failure worth preventing.
    engine.checkVaultHealth = async () => {
      throw {
        code: "notInitialized",
        message: "No vault folder is open yet. Choose a vault folder to continue.",
      };
    };
    harness = await renderWithApp(() => <App />, { engine });
    await waitFor(() => harness!.app.ready());

    expect(harness.app.failure()).toBeNull();
    // The one answer it could not get is missing, and everything else is there.
    expect(harness.app.health()).toBeNull();
    expect(harness.app.installs().length).toBeGreaterThan(0);
    expect(harness.app.plan()).not.toBeNull();
    expect(harness.app.library().length).toBeGreaterThan(0);
    expect(document.body.textContent).not.toContain("could not read the vault");
  });

  it("still shows the failure screen when a call fails for a real reason", async () => {
    const engine = new FixtureEngine();
    engine.checkVaultHealth = async () => {
      throw { code: "ioError", message: "The drive stopped responding." };
    };
    harness = await renderWithApp(() => <App />, { engine });
    await waitFor(() => harness!.app.ready());

    expect(harness.app.failure()).toBe("The drive stopped responding.");
    expect(document.body.textContent).toContain("could not read the vault");
  });
});

describe("a vault that really cannot be read", () => {
  it("still says so, because that one is a failure", async () => {
    const engine = new FixtureEngine();
    engine.getAppState = async () => {
      throw { code: "ioError", message: "The vault database could not be opened." };
    };
    harness = await renderWithApp(() => <App />, { engine });
    await waitFor(() => harness!.app.ready());

    expect(harness.app.failure()).toBe("The vault database could not be opened.");
    const text = document.body.textContent ?? "";
    expect(text).toContain("could not read the vault");
    expect(text).toContain("The vault database could not be opened.");
  });
});

describe("adding the very first install", () => {
  it("creates the vault the setup screen promised, and registers", async () => {
    const { app, engine } = await firstRun();
    expect(app.appState()!.vaultInitialized).toBe(false);

    await userEvent.click(
      screen.getByRole("button", { name: /Choose an install folder/ }),
    );
    await waitFor(() => screen.queryAllByRole("dialog").length > 0);
    await userEvent.click(screen.getByRole("button", { name: "Open C:\\" }));
    await waitFor(
      () => screen.queryAllByRole("button", { name: /Open ComfyUI-Alpha/ }).length > 0,
    );
    await userEvent.click(
      screen.getAllByRole("button").find((b) => b.textContent?.startsWith("ComfyUI-Alpha"))!,
    );
    await waitFor(() => document.querySelector(".verdict:not(.wait)") !== null);
    await userEvent.click(screen.getByRole("button", { name: "Add this install" }));

    await waitFor(() => app.installs().length > 0, 4000);
    // The vault exists now, and the install is registered in it.
    expect(app.appState()!.vaultInitialized).toBe(true);
    expect(app.vault()!.root).toBe("C:\\ComfyVault");
    expect((await engine.listInstalls()).length).toBe(1);
    // And the person is past the setup screen.
    expect(document.body.textContent).not.toContain("Nothing registered yet");
    expect(app.failure()).toBeNull();
  });

  it("never lets a refusal to that button go by unnoticed", async () => {
    const { app } = await firstRun();
    app.engine.registerInstall = async () => {
      throw { code: "notAComfyInstall", message: "That folder is not a ComfyUI install." };
    };

    await userEvent.click(
      screen.getByRole("button", { name: /Choose an install folder/ }),
    );
    await waitFor(() => screen.queryAllByRole("dialog").length > 0);
    await userEvent.click(screen.getByRole("button", { name: "Open C:\\" }));
    await waitFor(
      () => screen.queryAllByRole("button", { name: /Open ComfyUI-Alpha/ }).length > 0,
    );
    await userEvent.click(
      screen.getAllByRole("button").find((b) => b.textContent?.startsWith("ComfyUI-Alpha"))!,
    );
    await waitFor(() => document.querySelector(".verdict:not(.wait)") !== null);
    await userEvent.click(screen.getByRole("button", { name: "Add this install" }));

    // It stays in front of them, in the modal, beside the button they pressed.
    await waitFor(() => document.querySelector('[role="dialog"] .verdict.no') !== null);
    const dialog = document.querySelector('[role="dialog"]')!;
    expect(dialog.textContent).toContain("That did not happen");
    expect(dialog.textContent).toContain("That folder is not a ComfyUI install.");
    expect(screen.queryAllByRole("dialog").length).toBe(1);
  });
});

describe("the drive meter before anything has been read", () => {
  it("does not claim every model is already held once", async () => {
    const engine = new FixtureEngine({ empty: true });
    await engine.selectVault("C:\\ComfyVault");
    await engine.registerInstall("C:\\ComfyUI-Alpha");
    harness = await renderWithApp(() => <App />, { engine });
    await waitFor(() => harness!.app.ready());

    expect(harness.app.scan()).toBeNull();
    const rail = document.querySelector(".rail")!.textContent ?? "";
    // Nothing has been read, so there is nothing to say about duplicates.
    expect(rail).not.toContain("every model is held once");
    expect(rail).toContain("not scanned yet");
  });
});
