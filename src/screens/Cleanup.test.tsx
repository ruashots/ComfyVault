import { screen } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";

import { fmt } from "~/domain/format";
import { ConfirmModalView } from "~/modals/confirm";
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

  it("says what one model's names are, and where each is used, in a sentence", async () => {
    const { app } = await afterARun();
    await waitFor(() => app.nameGroups().length > 0);
    const group = app.nameGroups()[0]!;
    const card = document.querySelector(".cgrp")!;
    expect(card.querySelector(".ch")!.textContent).toContain(
      `${group.category}:the same file under ${group.names.length} names`,
    );
    const used = group.names.find((n) => n.usedByLinks > 0 && n.seenInInstalls.length > 0)!;
    const lines = [...card.querySelectorAll(".rs")].map((r) => r.textContent);
    expect(lines.some((l) => /^the name used in .+, where \d+ links? points? at it$/.test(l ?? ""))).toBe(
      true,
    );
    expect(used).toBeDefined();
    expect(card.textContent).not.toContain("·");
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
