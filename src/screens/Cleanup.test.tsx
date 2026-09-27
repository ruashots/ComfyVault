import { screen } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";

import { App } from "~/App";
import { fmt } from "~/domain/format";
import { ConfirmModalView } from "~/modals/confirm";
import { openUndoBox } from "~/modals/undo";
import { CleanupScreen, cleanupSummary } from "~/screens/Cleanup";
import { FixtureEngine } from "~/ipc/fixture/engine";
import { renderWithApp, waitFor, type Harness } from "~/test/render";

let harness: Harness | null = null;

afterEach(() => {
  harness?.unmount();
  harness = null;
});

/** Apply every group of the sample plan, then open Cleanup. */
async function afterARun(prepare?: (engine: FixtureEngine) => Promise<void> | void) {
  const engine = new FixtureEngine({ manual: true });
  engine.devSetSymlinksSupported(true);
  engine.devSetComfyRunning(false);
  const scan = (await engine.getLastScan())!;
  const plan = await engine.buildPlan(scan.scanId);
  await engine.startApply({
    planId: plan.planId,
    groupIds: plan.groups.map((g) => g.groupId),
  });
  engine.devFinish();
  await prepare?.(engine);
  harness = await renderWithApp(
    () => (
      <>
        <CleanupScreen />
        <ConfirmModalView />
      </>
    ),
    { engine },
  );
  await waitFor(() => harness!.app.vaultFiles().length > 0);
  return harness;
}

const titles = () => [...document.querySelectorAll(".sec .t")].map((t) => t.textContent);
const counts = () => [...document.querySelectorAll(".sec .n")].map((t) => t.textContent);

describe("the top line of Cleanup", () => {
  it("is made of sentences, not labels strung together", () => {
    expect(cleanupSummary(0, 1, 0)).toBe(
      "1 model has more than one name. Every vault file is linked.",
    );
    expect(cleanupSummary(2, 3, 1)).toBe(
      "2 links lead to nothing. 3 models have more than one name. 1 vault file is not linked from any install.",
    );
    expect(cleanupSummary(0, 0, 0, 1)).toBe(
      "1 delete stopped part way. Every model has one name. Every vault file is linked.",
    );
    expect(cleanupSummary(1, 0, 4)).toBe(
      "1 link leads to nothing. Every model has one name. 4 vault files are not linked from any install.",
    );
  });
});

describe("the Cleanup sections, after a run", () => {
  it("title each section with a phrase", async () => {
    const { app } = await afterARun();
    await waitFor(() => app.nameGroups().length > 0);
    expect(titles().slice(0, 2)).toEqual([
      "One model with more than one name",
      "Vault files that nothing links to",
    ]);
    expect(counts()[0]).toBe(
      `${app.nameGroups().length} ${app.nameGroups().length === 1 ? "model" : "models"}`,
    );
    const orphans = app.orphans();
    expect(counts()[1]).toBe(
      orphans.length === 0
        ? "none"
        : `${orphans.length} ${orphans.length === 1 ? "file" : "files"}, ${fmt(orphans.reduce((s, f) => s + f.sizeBytes, 0))}`,
    );
    for (const count of counts()) expect(count).not.toContain("·");
  });

  it("says of each name whether the vault file has it, and which installs use it", async () => {
    const { app, engine } = await afterARun();
    await waitFor(() => app.nameGroups().length > 0);
    const group = app.nameGroups()[0]!;
    const card = document.querySelector(".cgrp")!;
    expect(card.querySelector(".ch")!.textContent).toContain(
      `${group.category}:the same file under ${group.names.length} names`,
    );
    const links = await engine.listLinks({ sha256: group.sha256 });
    const rows = [...card.querySelectorAll(".optrow")];
    group.names.forEach((n, i) => {
      const ids = [...new Set(links.filter((l) => l.linkName === n.name).map((l) => l.installId))];
      const who = ids.map((id) => (id === "studio" ? "ComfyUI-Studio" : "ComfyUI-Sandbox"));
      const usedBy =
        who.length === 0
          ? "No install uses this name."
          : `${who.join(" and ")} ${who.length === 1 ? "uses" : "use"} this name.`;
      expect(rows[i]!.querySelector(".rs")!.textContent).toBe(
        `${n.isCanonical ? "The vault file has this name now." : "The vault keeps this name as a link beside the file."} ${usedBy}`,
      );
    });
    // Each name in the sample is used by the install whose link carries it.
    expect(card.textContent).not.toContain("a name you typed");
    expect(card.textContent).not.toContain("links point at it");
    expect(screen.queryByRole("button", { name: /Type a different name/ })).toBeNull();
    expect(card.textContent).toContain(
      "Choosing a name renames the file in the vault and keeps the other name as a link beside it. No install changes, and no disk space is freed.",
    );
    expect(card.textContent).not.toContain("·");
  });

  it("renames the vault file when another of its names is chosen", async () => {
    const { app, engine } = await afterARun();
    await waitFor(() => app.nameGroups().length > 0);
    const group = app.nameGroups()[0]!;
    const other = group.names.find((n) => !n.isCanonical)!;
    const radio = [...document.querySelectorAll('.cgrp')][0]!.querySelectorAll('[role="radio"]');
    const target = [...radio].find((r) => r.textContent?.includes(other.name)) as HTMLButtonElement;
    await userEvent.click(target);
    await waitFor(() => app.toast() !== null);
    expect(app.toast()!.message).toBe(`The vault file is now named ${other.name}.`);
    const file = (await engine.listVaultFiles({ offset: 0, limit: 1000 })).files.find((f) => f.sha256 === group.sha256)!;
    expect(file.canonicalName).toBe(other.name);
  });

  it("keeps each name's words true after another name is chosen", async () => {
    const { app } = await afterARun();
    await waitFor(() => app.nameGroups().length > 0);
    const group = app.nameGroups()[0]!;
    const before = group.canonicalName;
    const other = group.names.find((n) => !n.isCanonical)!.name;
    const card = () =>
      [...document.querySelectorAll(".cgrp")].find((c) => c.textContent!.includes(before))!;
    const rowOf = (name: string) =>
      [...card().querySelectorAll(".optrow")].find(
        (r) => r.querySelector(".nm")!.getAttribute("title") === name,
      )!;
    const words = (name: string) => rowOf(name).querySelector(".rs")!.textContent!;
    const usedBy = (name: string) => words(name).replace(/^The vault [^.]+\. /, "");
    const suggested = () =>
      [...card().querySelectorAll(".optrow")]
        .filter((r) => r.querySelector(".tag") !== null)
        .map((r) => r.querySelector(".nm")!.getAttribute("title"));
    const reasons = () => [...card().querySelectorAll(".why")].map((w) => w.textContent);
    const usersBefore = { [before]: usedBy(before), [other]: usedBy(other) };
    const suggestedBefore = suggested();
    const reasonsBefore = reasons();

    await userEvent.click(rowOf(other).querySelector('[role="radio"]')!);
    await waitFor(
      () => app.nameGroups().find((g) => g.sha256 === group.sha256)!.canonicalName === other,
    );

    expect(words(other)).toBe(`The vault file has this name now. ${usersBefore[other]}`);
    expect(words(before)).toBe(
      `The vault keeps this name as a link beside the file. ${usersBefore[before]}`,
    );
    // Choosing moves the vault's name, not the installs' links, so the
    // suggestion and the reason for it stay as they were.
    expect(suggested()).toEqual(suggestedBefore);
    expect(reasons()).toEqual(reasonsBefore);
    expect(card().textContent).not.toContain("a name you typed");
  });

  it("says a vault file nothing links to will be freed, not that it comes back", async () => {
    const { app } = await afterARun();
    await waitFor(() => app.orphans().length > 0);
    const orphan = app.orphans()[0]!;
    const row = [...document.querySelectorAll(".grp")].find(
      (g) => g.querySelector(".grp-n")?.textContent === orphan.canonicalName,
    )!;
    expect(row.querySelector(".grp-why")!.textContent).toMatch(
      new RegExp(`^in ${orphan.category}, in the vault since .+, and no install links to it$`),
    );
    (row.querySelector("button.dng") as HTMLButtonElement).click();
    await waitFor(() => document.querySelector(".modal") !== null);
    const modal = document.querySelector(".modal")!.textContent ?? "";
    expect(modal).toContain(`${orphan.canonicalName} will be deleted from the vault, and`);
    expect(modal).toContain(`${fmt(orphan.sizeBytes)} will be freed.`);
    expect(modal).not.toContain("comes back");
  });
});

const deleteRows = () => [...document.querySelectorAll(".vrow")];
const rowOf = (name: string) =>
  deleteRows().find((r) => r.querySelector(".grp-n")?.textContent === name);
const modalText = () => (document.querySelector(".modal")?.textContent ?? "").replace(/\s+/g, " ");

/** The linked models the list shows before "Show all". */
const shownModels = (app: Harness["app"]) =>
  app
    .vaultFiles()
    .filter((f) => f.linkCount > 0)
    .sort((a, b) => b.sizeBytes - a.sizeBytes)
    .slice(0, 12);

async function openDelete(name: string) {
  const bin = screen.getByRole("button", { name: `Delete ${name}` });
  await userEvent.click(bin);
  await waitFor(() => document.querySelector(".modal") !== null);
}

describe("deleting a model from Cleanup, with every link to it", () => {
  it("lists every linked model, biggest first, and not the ones nothing links to", async () => {
    const { app } = await afterARun();
    const linked = app.vaultFiles().filter((f) => f.linkCount > 0);
    expect(titles()).toContain("Delete a model");
    expect(counts()[titles().indexOf("Delete a model")]).toBe(
      `${linked.length} models in the vault, ${fmt(linked.reduce((s, f) => s + f.sizeBytes, 0))}`,
    );
    const names = deleteRows().map((r) => r.querySelector(".grp-n")!.textContent);
    expect(names).toHaveLength(12);
    const biggest = [...linked].sort((a, b) => b.sizeBytes - a.sizeBytes).slice(0, 12);
    expect(names).toEqual(biggest.map((f) => f.canonicalName));
    for (const orphan of app.orphans()) expect(names).not.toContain(orphan.canonicalName);

    await userEvent.click(screen.getByRole("button", { name: `Show all ${linked.length}` }));
    expect(deleteRows()).toHaveLength(linked.length);
    expect(screen.getByRole("button", { name: "Show the 12 biggest only" })).toBeDefined();
  });

  it("says where each model is linked and whether a saved workflow uses it", async () => {
    const { app } = await afterARun();
    const file = app.vaultFiles().filter((f) => f.linkCount > 0).sort((a, b) => b.sizeBytes - a.sizeBytes)[0]!;
    const row = rowOf(file.canonicalName)!;
    const meta = row.querySelector(".vmeta")!.textContent;
    expect(meta).toMatch(new RegExp(`^in ${file.category},linked in .+,used by (no saved workflow|\\d+ saved workflows?)$`));
    expect(row.querySelector(".lk")!.getAttribute("title")).toBe(file.links.map((l) => l.absPath).join("\n"));
    expect(row.querySelector(".grp-s")!.textContent).toBe(fmt(file.sizeBytes));
  });

  it("names every link, every install and every workflow before it deletes", async () => {
    const { app, engine } = await afterARun();
    const file = shownModels(app).find(
      (f) => f.linkCount >= 2 && new Set(f.links.map((l) => l.installId)).size === 2,
    )!;
    expect(file, "the sample must hold a model linked from both installs").toBeDefined();
    await openDelete(file.canonicalName);
    const text = modalText();
    expect(text).toContain(
      `${file.canonicalName} will be deleted, and ${fmt(file.sizeBytes)} will be freed on drive C:.`,
    );
    expect(text).toContain(
      "This is the only copy ComfyVault knows of. This cannot be undone. To use the model again, you must download it again.",
    );
    expect(text).toContain(
      `Its ${file.links.length} links are removed too, so it disappears from ComfyUI-Studio and ComfyUI-Sandbox:`,
    );
    const listed = [...document.querySelectorAll(".modal .mb > .paths li")].map((li) => li.textContent);
    expect(listed).toEqual(file.links.map((l) => l.absPath));
    // The usage line comes after the paths.
    const after = document.querySelector(".modal .mb > .paths")!.nextElementSibling!.textContent ?? "";
    expect(after).toMatch(/^(It is named in|No saved workflow names it)/);
    expect(screen.getByRole("button", { name: `Delete the model and its ${file.links.length} links` })).toBeDefined();
    // Nothing is removed until the person says so.
    expect((await engine.listLinks({ sha256: file.sha256 })).length).toBe(file.links.length);
  });

  it("deletes the model and every link, closes, and says what it freed", async () => {
    const { app, engine } = await afterARun();
    const file = shownModels(app).find((f) => f.linkCount >= 2)!;
    await openDelete(file.canonicalName);
    await userEvent.click(
      screen.getByRole("button", { name: `Delete the model and its ${file.links.length} links` }),
    );
    await waitFor(() => document.querySelector(".modal") === null);
    await waitFor(() => rowOf(file.canonicalName) === undefined);
    expect(app.toast()?.message).toBe(
      `Deleted ${file.canonicalName}. ${fmt(file.sizeBytes)} freed, ${file.links.length} links removed.`,
    );
    expect(await engine.listLinks({ sha256: file.sha256 })).toEqual([]);
    expect(app.vaultFiles().some((f) => f.sha256 === file.sha256)).toBe(false);
    expect(app.orphans().some((f) => f.sha256 === file.sha256)).toBe(false);
  });

  it("stays open with the engine's reason when the file is held open, and removes nothing", async () => {
    let held = "";
    const { app, engine } = await afterARun(async (e) => {
      const { files } = await e.listVaultFiles({ offset: 0, limit: 1000 });
      held = [...files].sort((a, b) => b.sizeBytes - a.sizeBytes).find((f) => f.linkCount >= 2)!.sha256;
      e.devHoldOpen(held);
    });
    const file = app.vaultFiles().find((f) => f.sha256 === held)!;
    await openDelete(file.canonicalName);
    await userEvent.click(screen.getByRole("button", { name: /^Delete the model and its/ }));
    await waitFor(() => document.querySelector(".modal .verdict") !== null);
    const verdict = document.querySelector(".modal .verdict")!.textContent ?? "";
    expect(verdict).toContain("Another program has this model open, so nothing was deleted.");
    expect(verdict).toContain(`C:\\ComfyVault\\${file.vaultRelPath}`);
    expect((await engine.listLinks({ sha256: file.sha256 })).length).toBe(file.links.length);
  });

  it("refuses when a link path now holds a real file, and names that path", async () => {
    let replaced = "";
    const { app, engine } = await afterARun(async (e) => {
      const { files } = await e.listVaultFiles({ offset: 0, limit: 1000 });
      const f = [...files].sort((a, b) => b.sizeBytes - a.sizeBytes).find((x) => x.linkCount >= 2)!;
      replaced = f.links[1]!.absPath;
      e.devReplaceLink(replaced);
    });
    const file = app.vaultFiles().find((f) => f.links.some((l) => l.absPath === replaced))!;
    await openDelete(file.canonicalName);
    await userEvent.click(screen.getByRole("button", { name: /^Delete the model and its/ }));
    await waitFor(() => document.querySelector(".modal .verdict") !== null);
    const verdict = document.querySelector(".modal .verdict")!;
    expect(verdict.textContent).toContain("Something else sits at these paths now.");
    expect([...verdict.querySelectorAll("li")].map((li) => li.textContent)).toEqual([replaced]);
    expect((await engine.listLinks({ sha256: file.sha256 })).length).toBe(file.links.length);
  });

  it("lists only models no saved workflow names, with the sentence that says what was checked", async () => {
    const { app } = await afterARun();
    const chip = screen.getByRole("button", { name: /^Only models no saved workflow uses/ });
    await userEvent.click(chip);
    expect(chip.getAttribute("aria-pressed")).toBe("true");
    expect(document.querySelector(".lib-method")!.textContent).toContain(
      "so this list is not a list of models that are safe to delete.",
    );
    for (const row of deleteRows()) {
      expect(row.querySelector(".wf")!.textContent).toBe("used by no saved workflow");
    }
    const n = Number(/\((\d+)\)/.exec(chip.textContent ?? "")![1]);
    expect(Math.min(n, 12)).toBe(deleteRows().length);
    expect(app.usage().size).toBeGreaterThan(0);
  });

  it("shows no usage and no filter when no saved workflow was searched", async () => {
    await afterARun((e) => e.devSetWorkflowsOnDisk(0));
    expect(screen.queryByRole("button", { name: /^Only models no saved workflow uses/ })).toBeNull();
    expect(document.querySelectorAll(".vrow .wf")).toHaveLength(0);
    const first = deleteRows()[0]!.querySelector(".lk")!.textContent;
    expect(first?.endsWith(",")).toBe(false);
  });

  it("says so when no model in the vault is linked", async () => {
    await afterARun(async (e) => {
      for (const link of await e.listLinks()) await e.removeLink(link.id);
    });
    expect(document.body.textContent).toContain("No model in the vault is linked from an install.");
    expect(deleteRows()).toHaveLength(0);
  });
});

describe("backing out of the delete", () => {
  it("closes on Escape and deletes nothing", async () => {
    const engine = new FixtureEngine({ manual: true });
    engine.devSetSymlinksSupported(true);
    engine.devSetComfyRunning(false);
    const scan = (await engine.getLastScan())!;
    const plan = await engine.buildPlan(scan.scanId);
    await engine.startApply({ planId: plan.planId, groupIds: plan.groups.map((g) => g.groupId) });
    engine.devFinish();
    // The whole window, because Escape is the window's key, not the dialog's.
    harness = await renderWithApp(() => <App />, { engine });
    harness.app.actions.go("cleanup");
    await waitFor(() => deleteRows().length > 0);
    const file = shownModels(harness.app).find((f) => f.linkCount >= 2)!;
    let deletes = 0;
    const original = engine.deleteVaultFile.bind(engine);
    engine.deleteVaultFile = async (...args) => {
      deletes += 1;
      return original(...args);
    };

    await openDelete(file.canonicalName);
    await userEvent.keyboard("{Escape}");
    await waitFor(() => document.querySelector(".modal") === null);

    expect(deletes).toBe(0);
    expect((await engine.listLinks({ sha256: file.sha256 })).length).toBe(file.links.length);
    expect(rowOf(file.canonicalName)).toBeDefined();
  });
});

describe("a delete that stopped part way", () => {
  async function withStoppedDelete() {
    let gone: string[] = [];
    let sha = "";
    const h = await afterARun(async (e) => {
      const { files } = await e.listVaultFiles({ offset: 0, limit: 1000 });
      const f = files.find((x) => x.linkCount >= 3) ?? files.find((x) => x.linkCount >= 2)!;
      sha = f.sha256;
      gone = e.devStopDelete(sha, 1);
    });
    await waitFor(() => (h.app.health()?.stoppedDeletes.length ?? 0) > 0);
    const file = h.app.vaultFiles().find((f) => f.sha256 === sha)!;
    return { ...h, file, gone };
  }

  it("is the first thing Cleanup says, with a way to finish it", async () => {
    const { file } = await withStoppedDelete();
    const panel = document.querySelector(".scroll > .blk")!;
    expect(panel.textContent).toContain("A delete stopped part way");
    expect(panel.textContent).toContain("Some installs already lost their link, so they no longer load it.");
    expect(panel.textContent).toContain(file.canonicalName);
    expect(panel.textContent).toContain(`in ${file.category}, ${fmt(file.sizeBytes)} still in the vault`);
    expect(screen.getByRole("button", { name: "Finish the delete" })).toBeDefined();
  });

  it("lists only the links still there, then finishes the delete", async () => {
    const { app, engine, file, gone } = await withStoppedDelete();
    await userEvent.click(screen.getByRole("button", { name: "Finish the delete" }));
    await waitFor(() => document.querySelector(".modal") !== null);
    const left = file.links.map((l) => l.absPath).filter((p) => !gone.includes(p));
    const listed = [...document.querySelectorAll(".modal .mb > .paths li")].map((li) => li.textContent);
    expect(listed).toEqual(left);
    expect(modalText()).toContain(
      `${file.canonicalName} will be deleted, and ${fmt(file.sizeBytes)} will be freed on drive C:.`,
    );
    expect(modalText()).toContain(
      left.length === 1 ? "Its last link is removed too" : `Its ${left.length} remaining links are removed too`,
    );

    const finish = [...document.querySelectorAll(".modal button")].find(
      (b) => b.textContent === "Finish the delete",
    ) as HTMLButtonElement;
    await userEvent.click(finish);
    await waitFor(() => document.querySelector(".modal") === null);
    await waitFor(() => (app.health()?.stoppedDeletes.length ?? 1) === 0);
    expect(document.querySelector(".scroll > .blk")).toBeNull();
    expect(app.toast()?.message).toBe(
      `Deleted ${file.canonicalName}. ${fmt(file.sizeBytes)} freed, ${left.length} ${left.length === 1 ? "link" : "links"} removed.`,
    );
    expect(await engine.listLinks({ sha256: file.sha256 })).toEqual([]);
  });
});

describe("the rest of the window, while a delete is stopped part way", () => {
  async function appWithStoppedDelete() {
    const engine = new FixtureEngine({ manual: true });
    engine.devSetSymlinksSupported(true);
    engine.devSetComfyRunning(false);
    const scan = (await engine.getLastScan())!;
    const plan = await engine.buildPlan(scan.scanId);
    await engine.startApply({ planId: plan.planId, groupIds: plan.groups.map((g) => g.groupId) });
    engine.devFinish();
    const { files } = await engine.listVaultFiles({ offset: 0, limit: 1000 });
    const gone = engine.devStopDelete(files.find((f) => f.linkCount >= 2)!.sha256, 1);
    harness = await renderWithApp(() => <App />, { engine });
    await waitFor(() => (harness!.app.health()?.stoppedDeletes.length ?? 0) > 0);
    return { h: harness, gone };
  }

  it("counts it on Cleanup's badge", async () => {
    const { h } = await appWithStoppedDelete();
    const app = h.app;
    const expected =
      app.danglingLinks().length + 1 + app.nameGroups().length + app.orphans().length;
    const cleanup = screen.getAllByRole("button").find((b) => /^Cleanup/.test(b.textContent ?? ""))!;
    expect(cleanup.textContent).toBe(`Cleanup${expected}`);
  });

  it("does not call the vault well when the links are checked", async () => {
    const { h } = await appWithStoppedDelete();
    h.app.actions.go("settings");
    await userEvent.click(await screen.findByRole("button", { name: /Check every link/ }));
    await waitFor(() => h.app.toast() !== null);
    expect(h.app.toast()!.message).toBe("A delete stopped part way. Finish it in Cleanup.");
    expect(h.app.screen()).toBe("cleanup");
  });

  it("says why the run cannot be undone", async () => {
    const { h, gone } = await appWithStoppedDelete();
    const applyId = h.app.lastApply()!.applyId;
    await openUndoBox(h.app, applyId);
    await waitFor(() => document.querySelector(".modal .verdict") !== null);
    const verdict = document.querySelector(".modal .verdict")!;
    expect(verdict.textContent).toContain(
      "A delete of one of this run's models stopped part way, so this run was not undone. Nothing was changed. Finish the delete in Cleanup. After that, this run can no longer be undone.",
    );
    expect([...verdict.querySelectorAll("li")].map((li) => li.textContent)).toEqual([gone.join(", ")]);
    expect(screen.queryByRole("button", { name: "Undo the run" })).toBeNull();
  });
});
