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

const WAIT = "Wait for the undo to finish";

/**
 * A run, then its undo, held part way. The engine makes every link and name
 * change wait for the undo, which can take minutes, so none may be offered.
 */
async function undoing(prepare?: (engine: FixtureEngine) => Promise<void>): Promise<Harness> {
  const engine = new FixtureEngine({ manual: true });
  engine.devSetSymlinksSupported(true);
  engine.devSetComfyRunning(false);
  // A link to a model the vault held before the run, then its file lost, so
  // the panel that removes broken links shows too.
  const [held] = await engine.listOrphans();
  await engine.createLink({ installId: "studio", sha256: held!.sha256, relativeDir: "models\\loras" });
  engine.devBreakLinks(1);
  const scan = (await engine.getLastScan())!;
  const plan = await engine.buildPlan(scan.scanId);
  const { applyId } = await engine.startApply({
    planId: plan.planId,
    groupIds: plan.groups.map((g) => g.groupId),
  });
  engine.devFinish();
  await prepare?.(engine);
  harness = await renderWithApp(() => <App />, { engine });
  await engine.revertApply(applyId);
  engine.devAdvance(1);
  await waitFor(() => harness!.app.undoRunning());
  return harness;
}

const disabled = (el: Element) => (el as HTMLButtonElement).disabled;

describe("while a consolidation is undone", () => {
  it("offers no link or delete in Cleanup, and says why", async () => {
    const { app } = await undoing();
    app.actions.go("cleanup");
    await waitFor(() => document.querySelectorAll(".vrow .drop").length > 0);

    // Deleting a model, with its links or with none.
    for (const b of document.querySelectorAll(".vrow .drop")) expect(disabled(b)).toBe(true);
    const deletes = screen.getAllByRole("button", { name: /^Delete$/ });
    expect(deletes.length).toBeGreaterThan(0);
    for (const b of deletes) expect(disabled(b)).toBe(true);
    // Removing a broken link.
    const removes = [
      ...screen.getAllByRole("button", { name: /^Remove (it|them all)$/ }),
      ...screen.getAllByRole("button", { name: /^Remove this one$/ }),
    ];
    for (const b of removes) expect(disabled(b)).toBe(true);
    const reasons = [...document.querySelectorAll(".undo-wait")].map((e) => e.textContent);
    expect(reasons).toEqual([`${WAIT}.`, `${WAIT}.`, `${WAIT}.`]);
  });

  it("offers no link into an install from the Library", async () => {
    const { app } = await undoing();
    const row = app.library()[0]!;
    app.setLib({ selected: row.sha256, drawerOpen: true });
    app.actions.go("library");
    const link = await waitForButton("Link into an install");
    expect(disabled(link)).toBe(true);
    expect(document.querySelector(".det-acts .undo-wait")!.textContent).toBe(`${WAIT}.`);
  });

  it("does not give a model one name until the undo ends", async () => {
    const { app, engine } = await undoing();
    // The name cards come from a moment before the undo started to put names back.
    const [group] = await engine.listNameGroups();
    app.setModal({ kind: "unify", sha256: group!.sha256, name: group!.names[1]!.name, plan: null, working: false, error: null });
    app.patchModal((m) => {
      if (m.kind === "unify") m.plan = { sha256: group!.sha256, name: group!.names[1]!.name, links: [], running: [], workflows: [] };
    });
    const primary = await waitForButton(WAIT);
    expect(disabled(primary)).toBe(true);
  });

  it("lets the buttons back once the undo is done", async () => {
    const { app, engine } = await undoing();
    app.actions.go("cleanup");
    await waitFor(() => document.querySelector(".undo-wait") !== null);
    engine.devFinish();
    await waitFor(() => !app.undoRunning());
    await waitFor(() => document.querySelector(".undo-wait") === null);
    const removes = screen.getAllByRole("button", { name: /^Remove this one$/ });
    for (const b of removes) expect(disabled(b)).toBe(false);
  });
});

async function waitForButton(name: string): Promise<HTMLElement> {
  await waitFor(() => screen.queryAllByRole("button", { name }).length > 0);
  return screen.getAllByRole("button", { name })[0]!;
}

const T5 = "https://huggingface.co/comfyanonymous/flux_text_encoders/blob/main/t5xxl_fp16.safetensors";

describe("adding links to a held model from Download, while an undo runs", () => {
  it("waits with the reason instead of adding them", async () => {
    const { app } = await undoing(async (engine) => {
      // Sandbox lacks its link to a model the vault holds.
      const t5 = (await engine.listVaultFiles({ offset: 0, limit: 1000 })).files.find(
        (f) => f.canonicalName === "t5xxl_fp16.safetensors",
      )!;
      await engine.removeLink(t5.links.find((l) => l.installId === "sandbox")!.id);
    });
    app.actions.go("download");
    const field = await waitFor(() => screen.queryAllByRole("textbox").length > 0).then(
      () => screen.getByRole("textbox", { name: /The address of a model/ }),
    );
    await userEvent.type(field, T5);
    await userEvent.click(screen.getByRole("button", { name: "Read the address" }));
    const wait = await waitForButton(WAIT);
    expect(disabled(wait)).toBe(true);
    expect(screen.queryByRole("button", { name: /^Add 1 link$/ })).toBeNull();
  });
});
