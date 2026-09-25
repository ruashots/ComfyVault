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

/** Walk the picker to a folder and take it. */
async function pickFolder(name: string, cta: string): Promise<void> {
  await waitFor(() => screen.queryAllByRole("dialog").length > 0);
  await userEvent.click(screen.getByRole("button", { name: "Open C:\\" }));
  await waitFor(
    () => screen.queryAllByRole("button", { name: new RegExp(`Open ${name}`) }).length > 0,
  );
  await userEvent.click(
    screen.getAllByRole("button").find((b) => b.textContent?.startsWith(name))!,
  );
  await waitFor(() => document.querySelector(".verdict:not(.wait)") !== null);
  await userEvent.click(screen.getByRole("button", { name: cta }));
}

const addInstall = () =>
  userEvent.click(screen.getByRole("button", { name: /Choose an install folder/ }));
const chooseVault = () =>
  userEvent.click(screen.getByRole("button", { name: /Choose the vault folder/ }));

describe("the setup screen", () => {
  it("puts the vault first, because an install cannot be recorded without one", async () => {
    await firstRun();
    const rows = [...document.querySelectorAll(".setup .stt")].map(
      (el) => el.textContent,
    );
    expect(rows).toEqual(["Where the vault goes", "Your ComfyUI installs"]);
  });

  it("shows both things to set, neither done, and calls nothing a failure", async () => {
    const { app } = await firstRun();
    expect(app.failure()).toBeNull();
    expect(app.setupDone()).toBe(false);

    const text = document.body.textContent ?? "";
    expect(text).toContain("Set ComfyVault up");
    expect(text).toContain("Two things to set");
    expect(text).toContain("Your ComfyUI installs");
    expect(text).toContain("Where the vault goes");
    expect(text).toContain("not chosen yet");
    expect(text).toContain("nothing set yet");

    // Neither step is ticked.
    expect(document.querySelectorAll(".setup.done")).toHaveLength(0);
    // And none of the old apology.
    expect(text).not.toContain("could not read the vault");
    expect(screen.queryByRole("button", { name: "Try again" })).toBeNull();
  });

  it("says what happens after setup, so nothing moving is not a surprise", async () => {
    await firstRun();
    const text = document.body.textContent ?? "";
    expect(text).toContain("What happens after this");
    expect(text).toContain("nothing moves on its own");
    expect(text).toContain("A scan reads every model file");
    expect(text).toContain("You get a plan to read");
    expect(text).toContain("Nothing moves until you press Apply");
  });

  it("says to put the vault with the installs, not on the drive with room", async () => {
    await firstRun();
    const text = document.body.textContent ?? "";
    expect(text).toContain("Put the vault on the same drive as your ComfyUI installs");
    expect(text).toContain("moved there rather than copied");
    expect(text).toContain("needs no free space of its own");
    expect(text).toContain("that drive needs the room up front");
    // And the kind of drive, which free space does not decide.
    expect(text).toContain("Not a removable or network drive");
    expect(text).toContain("stops loading at once");
  });
});

describe("the order the two steps come in", () => {
  it("cannot register an install until the vault exists", async () => {
    await firstRun();
    // Step two is visibly there and visibly not available yet, so the order
    // explains itself rather than the button simply being absent.
    const button = screen.getByRole("button", { name: /Choose an install folder/ });
    expect(button).toBeDisabled();
    expect(button.getAttribute("title")).toContain(
      "The vault has to exist before an install can point into it",
    );
    expect(document.body.textContent).toContain("waiting for the vault folder");
  });

  it("opens step two the moment the vault folder is chosen", async () => {
    const { app, engine } = await firstRun();
    await chooseVault();
    await pickFolder("ComfyVault", "Use this folder");
    await waitFor(() => app.hasVault());

    expect(app.setupDone()).toBe(false);
    const text = document.body.textContent ?? "";
    expect(text).toContain("1 of 2 done");
    expect(text).toContain("none registered yet");
    expect(text).not.toContain("waiting for the vault folder");
    expect(document.querySelectorAll(".setup.done")).toHaveLength(1);
    // The vault is real on disk, not held in this window.
    expect((await engine.getVaultInfo()).root).toBe("C:\\ComfyVault");

    await addInstall();
    await pickFolder("ComfyUI-Studio", "Add this install");
    await waitFor(() => app.setupDone(), 4000);
    expect((await engine.listInstalls()).length).toBe(1);
    expect(document.body.textContent).not.toContain("Two things to set");
  });

  it("every tick means something is on disk", async () => {
    const { app, engine } = await firstRun();
    await chooseVault();
    await pickFolder("ComfyVault", "Use this folder");
    await waitFor(() => app.hasVault());
    // A tick that only lived in this window would be a promise the disk is not
    // keeping, in an application whose whole point is not losing files.
    expect((await engine.getAppState()).vaultInitialized).toBe(true);
    expect(app.hasInstalls()).toBe(false);
    expect(document.querySelectorAll(".setup.done")).toHaveLength(1);
  });
});

describe("the screens that need both things set", () => {
  it.each(["library", "consolidate", "cleanup"] as const)(
    "%s says which step is missing rather than showing nothing",
    async (screenName) => {
      const { app } = await firstRun();
      app.actions.go(screenName);
      await waitFor(() => document.querySelector(".empty") !== null);

      const text = document.body.textContent ?? "";
      expect(text).toContain(
        "Choose where the vault goes, then register a ComfyUI install.",
      );
      expect(
        screen.getByRole("button", { name: /Finish setting up/ }),
      ).toBeInTheDocument();
    },
  );

  it("names only the step that is actually missing", async () => {
    const { app } = await firstRun();
    await chooseVault();
    await pickFolder("ComfyVault", "Use this folder");
    await waitFor(() => app.hasVault());

    app.actions.go("consolidate");
    await waitFor(() => document.querySelector(".empty") !== null);
    const text = document.body.textContent ?? "";
    expect(text).toContain("Register a ComfyUI install. The vault folder is already set.");
    expect(text).not.toContain("then register");
  });

  it("goes back to setup when the button is pressed", async () => {
    const { app } = await firstRun();
    app.actions.go("cleanup");
    await waitFor(() => document.querySelector(".empty") !== null);
    await userEvent.click(screen.getByRole("button", { name: /Finish setting up/ }));
    await waitFor(() => app.screen() === "home");
    expect(document.body.textContent).toContain("Two things to set");
  });
});

describe("Settings before a vault folder exists", () => {
  it("leaves no heading with nothing under it", async () => {
    const { app } = await firstRun();
    app.actions.go("settings");
    await waitFor(() => document.body.textContent?.includes("The vault folder") === true);

    const text = document.body.textContent ?? "";
    expect(text).toContain("Not chosen yet");
    expect(screen.getByRole("button", { name: /Choose it/ })).toBeInTheDocument();
  });

  it("does not claim the drive has no room on it", async () => {
    const { app } = await firstRun();
    app.actions.go("settings");
    await waitFor(() => document.body.textContent?.includes("Free space") === true);
    const text = document.body.textContent ?? "";
    expect(text).not.toContain("0 MB free");
    expect(text).toContain("not known until a vault folder is chosen");
  });
});

describe("one command that becomes conditional later", () => {
  it("costs that one answer, not the whole window", async () => {
    const engine = new FixtureEngine();
    engine.checkVaultHealth = async () => {
      throw {
        code: "notInitialized",
        message: "No vault folder is open yet. Choose a vault folder to continue.",
      };
    };
    harness = await renderWithApp(() => <App />, { engine });
    await waitFor(() => harness!.app.ready());

    expect(harness.app.failure()).toBeNull();
    expect(harness.app.health()).toBeNull();
    expect(harness.app.installs().length).toBeGreaterThan(0);
    expect(harness.app.plan()).not.toBeNull();
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

describe("a refusal to a button the person pressed", () => {
  it("stays in the modal beside it", async () => {
    const { app } = await firstRun();
    app.engine.selectVault = async () => {
      throw { code: "conflict", message: "That folder sits inside a ComfyUI install." };
    };
    await chooseVault();
    await pickFolder("ComfyVault", "Use this folder");

    await waitFor(() => document.querySelector('[role="dialog"] .verdict.no') !== null);
    const dialog = document.querySelector('[role="dialog"]')!;
    expect(dialog.textContent).toContain("That did not happen");
    expect(dialog.textContent).toContain("sits inside a ComfyUI install");
  });
});

describe("the drive meter before anything has been read", () => {
  it("does not claim every model is already held once", async () => {
    const engine = new FixtureEngine({ empty: true });
    await engine.selectVault("C:\\ComfyVault");
    await engine.registerInstall("C:\\ComfyUI-Studio");
    harness = await renderWithApp(() => <App />, { engine });
    await waitFor(() => harness!.app.ready());

    expect(harness.app.scan()).toBeNull();
    const rail = document.querySelector(".rail")!.textContent ?? "";
    expect(rail).not.toContain("every model is held once");
    expect(rail).toContain("not scanned yet");
  });
});

describe("the rail while the vault folder is still being chosen", () => {
  it("lists the drives with their real numbers, and no meter", async () => {
    const { app } = await firstRun();
    expect(app.drives().length).toBeGreaterThan(1);

    const rail = document.querySelector(".rail")!.textContent ?? "";
    expect(rail).toContain("Drives");
    expect(rail).toContain("C:");
    expect(rail).toContain("D:");
    expect(rail).toContain("free");
    // Nothing is measured against a drive nobody has chosen.
    expect(document.querySelector(".rail .meter")).toBeNull();
    expect(rail).not.toContain("0 MB free");
  });

  it("says a drive it cannot read is not readable, rather than empty", async () => {
    await firstRun();
    const rail = document.querySelector(".rail")!.textContent ?? "";
    // An empty card reader has no size. Zero of zero would read as full.
    expect(rail).toContain("not readable");
    expect(rail).not.toContain("0 B free");
  });

  it("collapses to the vault's own drive once one is chosen", async () => {
    const { app } = await firstRun();
    await chooseVault();
    await pickFolder("ComfyVault", "Use this folder");
    await waitFor(() => app.hasVault());

    const rail = document.querySelector(".rail")!.textContent ?? "";
    expect(rail).not.toContain("Drives");
    expect(rail).toContain("Drive C:");
    expect(document.querySelector(".rail .meter")).not.toBeNull();
  });
});

describe("what kind of drive the vault goes on", () => {
  it("names the kind in the rail, not just the room", async () => {
    await firstRun();
    const rail = document.querySelector(".rail")!.textContent ?? "";
    // Free space does not decide this, so the kind is said where the choice is.
    expect(rail).toContain("removable drive");
    expect(rail).toContain("network drive");
    expect(rail).toContain("not readable");
  });

  it("warns on a drive that can go away, and still allows it", async () => {
    await firstRun();
    await chooseVault();
    await waitFor(() => screen.queryAllByRole("dialog").length > 0);
    await userEvent.click(
      screen.getAllByRole("button").find((b) => b.textContent?.startsWith("E:\\"))!,
    );
    await waitFor(() => document.querySelector(".verdict:not(.wait)") !== null);

    const verdict = document.querySelector(".verdict:not(.wait)")!;
    expect(verdict.classList.contains("warn")).toBe(true);
    expect(verdict.textContent).toContain("This drive can go away");
    expect(verdict.textContent).toContain("every model in every install stops loading");
    expect(verdict.textContent).toContain("Free space is not what decides this");
    // The choice stays theirs, and the button says what taking it means.
    const confirm = screen.getByRole("button", { name: "Use it anyway" });
    expect(confirm).toBeEnabled();
    expect(confirm.classList.contains("dng")).toBe(true);
  });

  it("refuses a drive it cannot read, and will not make a folder there", async () => {
    await firstRun();
    await chooseVault();
    await waitFor(() => screen.queryAllByRole("dialog").length > 0);
    await userEvent.click(
      screen.getAllByRole("button").find((b) => b.textContent?.startsWith("Z:\\"))!,
    );
    await waitFor(() => document.querySelector(".verdict:not(.wait)") !== null);

    const verdict = document.querySelector(".verdict:not(.wait)")!;
    expect(verdict.classList.contains("no")).toBe(true);
    expect(verdict.textContent).toContain("This drive cannot be read");
    expect(verdict.textContent).toContain("network drive");
    expect(screen.queryByRole("button", { name: "Use it anyway" })).toBeNull();
    expect(screen.getByRole("button", { name: /Use this folder/ })).toBeDisabled();
    // It cannot create a folder on a drive it has just said it cannot see.
    const newFolder = screen.getByRole("button", { name: /New folder/ });
    expect(newFolder).toBeDisabled();
    expect(newFolder.getAttribute("title")).toContain("That drive cannot be read");
  });

  it("accepts an ordinary drive with no warning", async () => {
    await firstRun();
    await chooseVault();
    await waitFor(() => screen.queryAllByRole("dialog").length > 0);
    await userEvent.click(screen.getByRole("button", { name: "Open C:\\" }));
    await waitFor(
      () => screen.queryAllByRole("button", { name: /Open ComfyVault/ }).length > 0,
    );
    await userEvent.click(
      screen.getAllByRole("button").find((b) => b.textContent?.startsWith("ComfyVault"))!,
    );
    await waitFor(() => document.querySelector(".verdict:not(.wait)") !== null);

    const verdict = document.querySelector(".verdict:not(.wait)")!;
    expect(verdict.classList.contains("ok")).toBe(true);
    expect(verdict.classList.contains("warn")).toBe(false);
    expect(verdict.textContent).toContain("Drive C: accepted");
    expect(screen.getByRole("button", { name: /Use this folder/ })).toBeEnabled();
  });
});

describe("Settings before a vault, listing the drives", () => {
  it("lists only the ones that answered, and marks the odd kinds", async () => {
    const { app } = await firstRun();
    app.actions.go("settings");
    await waitFor(() => document.body.textContent?.includes("The vault folder") === true);

    const text = document.body.textContent ?? "";
    expect(text).toContain("C: has");
    expect(text).toContain("(removable drive)");
    // The drive that did not answer has no figure, so it is not listed.
    expect(text).not.toContain("Z: has");
    expect(text).not.toContain("undefined");
    expect(text).not.toContain("NaN");
  });
});
