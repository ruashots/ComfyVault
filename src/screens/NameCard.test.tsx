import { screen } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import { App } from "~/App";
import { FixtureEngine } from "~/ipc/fixture/engine";
import type { NameGroup } from "~/ipc/contract";
import { renderWithApp, waitFor, type Harness } from "~/test/render";

let harness: Harness | null = null;
afterEach(() => {
  harness?.unmount();
  harness = null;
});

/** Consolidate everything, so the installs link models under the names they had. */
async function consolidated(): Promise<FixtureEngine> {
  const engine = new FixtureEngine({ manual: true });
  engine.devSetSymlinksSupported(true);
  engine.devSetComfyRunning(false);
  const scan = (await engine.getLastScan())!;
  const plan = await engine.buildPlan(scan.scanId);
  await engine.startApply({ planId: plan.planId, groupIds: plan.groups.map((g) => g.groupId) });
  engine.devFinish();
  return engine;
}

/** Open Cleanup over that engine. */
async function cleanup(engine: FixtureEngine): Promise<Harness> {
  harness = await renderWithApp(() => <App />, { engine });
  harness.app.actions.go("cleanup");
  await waitFor(() => document.querySelector(".nc") !== null);
  return harness;
}

const cards = () => [...document.querySelectorAll(".nc")];
const text = (el: Element | null | undefined) => (el?.textContent ?? "").replace(/\s+/g, " ").trim();
const dialog = () => document.querySelector('[role="dialog"]');
const heading = () => text(dialog()?.querySelector("h2"));

/** The first card's model, whose names the sample installs split between them. */
async function firstGroup(engine: FixtureEngine): Promise<NameGroup> {
  return (await engine.listNameGroups())[0]!;
}

const SCALED = "umt5_xxl_fp8_e4m3fn_scaled.safetensors";
const ENC = "umt5-xxl-enc-fp8_e4m3fn.safetensors";

async function pickAndReview(name: string): Promise<void> {
  const card = cards()[0]!;
  const row = [...card.querySelectorAll('[role="radio"]')].find(
    (r) => r.querySelector(".nm")!.getAttribute("title") === name,
  )!;
  await userEvent.click(row);
  await userEvent.click(screen.getAllByRole("button", { name: "Review name change" })[0]!);
  await waitFor(() => dialog()?.querySelector(".mf .btn.pri") !== null);
}

describe("the card for one model with two names", () => {
  it("shows each name the installs use, with the installs that use it, and picks one", async () => {
    const engine = await consolidated();
    const { app } = await cleanup(engine);
    const group = await firstGroup(engine);
    expect(group.names.map((n) => n.name)).toEqual([SCALED, ENC]);

    const card = cards()[0]!;
    expect(text(card.querySelector(".nc-h"))).toBe("ONE MODEL, TWO NAMES");
    expect(text(card.querySelector(".nc-s"))).toBe("Choose the name to use in every install.");
    const rows = [...card.querySelectorAll('[role="radio"]')].map((r) => [
      r.querySelector(".nm")!.getAttribute("title"),
      text(r.querySelector(".who")),
      r.getAttribute("aria-checked"),
    ]);
    // One install each, one link each: the longer name is picked at the start.
    expect(rows).toEqual([
      [SCALED, "ComfyUI-Studio", "true"],
      [ENC, "ComfyUI-Sandbox", "false"],
    ]);
    expect(cards()).toHaveLength(app.nameCards().length);
    expect(text(document.querySelector(".hdr .sub") ?? document.querySelector("header"))).toContain(
      `${app.nameCards().length} models have more than one name in your installs.`,
    );
  });

  it("only selects a name when a row is clicked, and asks the engine nothing", async () => {
    const engine = await consolidated();
    await cleanup(engine);
    const plan = vi.spyOn(engine, "planUnifyName");
    const unify = vi.spyOn(engine, "unifyName");
    const hide = vi.spyOn(engine, "setHiddenNameCards");
    const enc = [...cards()[0]!.querySelectorAll('[role="radio"]')][1]!;
    await userEvent.click(enc);
    expect(enc.getAttribute("aria-checked")).toBe("true");
    expect(plan).not.toHaveBeenCalled();
    expect(unify).not.toHaveBeenCalled();
    expect(hide).not.toHaveBeenCalled();
  });
});

describe("reviewing the name change", () => {
  it("asks to use the picked name in both installs, and lists the workflows to fix", async () => {
    const engine = await consolidated();
    await cleanup(engine);
    await pickAndReview(ENC);
    expect(heading()).toBe("Use this name in both installs?");
    const body = dialog()!.querySelector(".mb")!;
    expect(text(body.querySelector(".fnbox"))).toBe(ENC);
    expect(text(body)).toContain(
      `Saved workflows using ${SCALED} will show a missing model until you pick this name.`,
    );
    expect(text(body)).toContain("Workflows to fix:");
    expect([...body.querySelectorAll(".wfs li")].map((li) => text(li))).toEqual([
      "• flow-1.json",
      "• flow-2.json",
      "• flow-3.json",
      "• flow-4.json",
    ]);
    expect(text(dialog()!.querySelector(".mf .btn.pri"))).toBe("Use this name");
  });

  it("says there are no saved workflows to fix when none uses the name that goes", async () => {
    const engine = await consolidated();
    await cleanup(engine);
    // The sample's workflows are all in ComfyUI-Studio, which keeps its name here.
    await pickAndReview(SCALED);
    expect(text(dialog()!.querySelector(".mb"))).toContain("No saved workflows to fix.");
    expect(dialog()!.querySelector(".wfs")).toBeNull();
  });

  it("asks to close a running ComfyUI first, and checks again", async () => {
    const engine = await consolidated();
    await cleanup(engine);
    engine.devSetRunningInstalls(["studio"]);
    await userEvent.click(
      [...cards()[0]!.querySelectorAll('[role="radio"]')][1]!,
    );
    await userEvent.click(screen.getAllByRole("button", { name: "Review name change" })[0]!);
    await waitFor(() => heading() === "Close ComfyUI-Studio first");
    expect(text(dialog()!.querySelector(".mb"))).toBe(
      "The name can change after ComfyUI-Studio is closed.",
    );
    // Still running: it stays.
    await userEvent.click(screen.getByRole("button", { name: "Check again" }));
    await waitFor(() => !(dialog()!.querySelector(".mf .btn.pri") as HTMLButtonElement).disabled);
    expect(heading()).toBe("Close ComfyUI-Studio first");
    // Closed: the confirm.
    engine.devSetRunningInstalls([]);
    await userEvent.click(screen.getByRole("button", { name: "Check again" }));
    await waitFor(() => heading() === "Use this name in both installs?");
  });

  it("names only the install that changes when the name is taken in the other", async () => {
    const engine = await consolidated();
    await cleanup(engine);
    const group = await firstGroup(engine);
    // Studio also links the model under the other name, in a second folder.
    await engine.createLink({ installId: "studio", sha256: group.sha256, relativeDir: "models\\clip", linkName: ENC });
    // And in Sandbox, some other file already has the picked name.
    const plan = await engine.planUnifyName(group.sha256, SCALED);
    const sandbox = plan.links.find((s) => s.installId === "sandbox")!;
    engine.devTakePath(sandbox.newAbsPath!);
    await harness!.app.actions.refresh();

    await pickAndReview(SCALED);
    expect(heading()).toBe("Use this name in ComfyUI-Studio?");
    expect(dialog()!.querySelector("h2 b")!.textContent).toBe("ComfyUI-Studio");
    const body = text(dialog()!.querySelector(".mb"));
    const aside = dialog()!.querySelector(".nc-aside")!;
    expect(aside.childNodes[0]!.textContent).toBe(
      "ComfyUI-Sandbox already uses this name for another file.",
    );
    expect(aside.lastChild!.textContent).toBe("Its model name will stay as it is.");
    expect(body).toContain(
      `Saved workflows using ${ENC} in ComfyUI-Studio will show a missing model until you pick this name.`,
    );
    expect(text(dialog()!.querySelector(".mf .btn.pri"))).toBe("Change name in ComfyUI-Studio");

    await userEvent.click(screen.getByRole("button", { name: "Change name in ComfyUI-Studio" }));
    await waitFor(() => dialog() === null);
    expect(text(document.querySelector(".nres p"))).toBe(
      "Name changed in ComfyUI-Studio. ComfyUI-Sandbox kept its name.",
    );
    // Sandbox still uses the other name, but the person has settled it: the card goes.
    await waitFor(() => !harness!.app.nameCards().some((c) => c.sha256 === group.sha256));
    expect(harness!.app.toast()).toBeNull();
  });
});

describe("once the name has changed", () => {
  it("takes the card off Cleanup and says so in a line at the top, with no toast", async () => {
    const engine = await consolidated();
    const { app } = await cleanup(engine);
    const group = await firstGroup(engine);
    const before = cards().length;
    await pickAndReview(ENC);
    await userEvent.click(screen.getByRole("button", { name: "Use this name" }));
    await waitFor(() => dialog() === null);
    await waitFor(() => cards().length === before - 1);

    expect(text(document.querySelector(".nres p"))).toBe("Name changed in ComfyUI-Studio.");
    expect(app.toast()).toBeNull();
    // Every link has the name now, and so has the vault file.
    const links = await engine.listLinks({ sha256: group.sha256 });
    expect(new Set(links.map((l) => l.linkName))).toEqual(new Set([ENC]));
    const file = (await engine.listVaultFiles({ offset: 0, limit: 1000 })).files.find(
      (f) => f.sha256 === group.sha256,
    )!;
    expect(file.canonicalName).toBe(ENC);
    expect(file.aliases).toEqual([]);

    await userEvent.click(screen.getByRole("button", { name: "View models" }));
    expect(app.screen()).toBe("library");
  });

  it("keeps the dialog open with the engine's reason when it refuses", async () => {
    const engine = await consolidated();
    await cleanup(engine);
    await pickAndReview(ENC);
    engine.devSetRunningInstalls(["studio"]);
    await userEvent.click(screen.getByRole("button", { name: "Use this name" }));
    await waitFor(() => dialog()?.querySelector(".verdict.no") != null);
    expect(text(dialog()!.querySelector(".verdict.no p"))).toBe("Close ComfyUI-Studio first");
    expect(cards().length).toBeGreaterThan(0);
  });
});

describe("keeping the names as they are", () => {
  it("hides the card, offers Undo, and keeps it hidden after a restart until a new name appears", async () => {
    const engine = await consolidated();
    const { app } = await cleanup(engine);
    const group = await firstGroup(engine);
    const before = cards().length;
    const summaryBefore = before;

    await userEvent.click(screen.getAllByRole("button", { name: "Keep names as they are" })[0]!);
    await waitFor(() => cards().length === before - 1);
    expect(app.toast()!.message).toBe("Kept the names as they are.");
    expect(app.nameCards().length).toBe(summaryBefore - 1);

    // Undo brings it back.
    await userEvent.click(screen.getByRole("button", { name: "Undo" }));
    await waitFor(() => cards().length === before);

    // Hidden again, then the app starts over the same engine.
    await userEvent.click(screen.getAllByRole("button", { name: "Keep names as they are" })[0]!);
    await waitFor(() => cards().length === before - 1);
    harness!.unmount();
    harness = await renderWithApp(() => <App />, { engine });
    harness.app.actions.go("cleanup");
    await waitFor(() => document.querySelector(".nc") !== null);
    expect(cards().length).toBe(before - 1);
    expect(harness.app.nameCards().some((c) => c.sha256 === group.sha256)).toBe(false);

    // An install starts using a third name: the card shows again.
    await engine.createLink({
      installId: "studio",
      sha256: group.sha256,
      relativeDir: "models\\clip",
      linkName: "umt5_third.safetensors",
    });
    await harness.app.actions.refresh();
    await waitFor(() => cards().length === before);
    expect(text(cards()[0]!.querySelector(".nc-h"))).toBe("ONE MODEL, THREE NAMES");
  });
});
