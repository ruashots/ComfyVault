import { afterEach, describe, expect, it, vi } from "vitest";

import { App } from "~/App";
import { fmt } from "~/domain/format";
import { FixtureEngine } from "~/ipc/fixture/engine";
import { renderWithApp, waitFor, type Harness } from "~/test/render";

let harness: Harness | null = null;
afterEach(() => {
  harness?.unmount();
  harness = null;
});

const tile = (label: string) =>
  [...document.querySelectorAll(".tile")].find((t) => t.querySelector(".k")!.textContent === label)!;
const value = (label: string) => tile(label).querySelector(".v")!.textContent;
const note = (label: string) => tile(label).querySelector(".x")!.textContent;
const tilesShown = () => document.querySelector(".tile") !== null;
const hero = () => document.querySelector(".hero")!.textContent ?? "";

/** Open Home over the sample installs, with a scan and no run yet. */
async function home(): Promise<Harness> {
  const engine = new FixtureEngine({ manual: true });
  engine.devSetSymlinksSupported(true);
  engine.devSetComfyRunning(false);
  harness = await renderWithApp(() => <App />, { engine });
  await waitFor(tilesShown);
  return harness;
}

/** Run the whole plan, then scan again, as the person did. */
async function consolidateAndRescan(h: Harness): Promise<void> {
  const plan = h.app.plan()!;
  await h.engine.startApply({ planId: plan.planId, groupIds: plan.groups.map((g) => g.groupId) });
  h.engine.devFinish();
  await h.engine.startScan();
  h.engine.devFinish();
  await h.app.actions.refresh();
  await waitFor(() => !h.app.scanPredatesRun());
  h.app.actions.go("home");
  await waitFor(tilesShown);
}

describe("Home's cards", () => {
  it("before a run, count the models in the installs and nothing in the Library", async () => {
    const h = await home();
    const t = h.app.scan()!.totals;
    expect(value("Installs")).toBe(String(h.app.installs().length));
    expect(value("Models still in installs")).toBe(String(t.uniqueContents));
    expect(note("Models still in installs")).toBe(
      `${t.movableFiles} files, ${fmt(t.movableBytes)}, not in the vault yet`,
    );
    // The sample vault already holds a few downloaded models.
    const vault = await h.engine.listVaultFiles({ offset: 0, limit: 1000 });
    expect(value("Models in the Library")).toBe(String(vault.total));
    expect(note("Models in the Library")).toBe(
      `${fmt(vault.files.reduce((sum, f) => sum + f.sizeBytes, 0))} in the vault`,
    );
    expect(note("Not used")).toBe("no saved workflow names them, so they can probably be deleted");
    expect(hero()).toContain("Review the plan");
  });

  it("after a run, count what is left in the installs and what the vault holds", async () => {
    const h = await home();
    const before = h.app.scan()!.totals;
    await consolidateAndRescan(h);
    const after = h.app.scan()!.totals;
    const vault = await h.engine.listVaultFiles({ offset: 0, limit: 1000 });
    expect(vault.total).toBeGreaterThan(0);

    // What the new scan still found to consolidate.
    expect(value("Models still in installs")).toBe(String(after.uniqueContents));
    expect(note("Models still in installs")).toBe(
      `${after.movableFiles} files, ${fmt(after.movableBytes)}, not in the vault yet`,
    );
    // Everything the vault holds.
    expect(value("Models in the Library")).toBe(String(vault.total));
    expect(note("Models in the Library")).toBe(
      `${fmt(vault.files.reduce((sum, f) => sum + f.sizeBytes, 0))} in the vault`,
    );
    for (const row of document.querySelectorAll(".inst")) {
      expect(Number(row.querySelector(".c1")!.textContent)).toBeGreaterThan(after.movableFiles);
    }
    // The installs still have every model they had, as a file or a link.
    expect(document.querySelector(".act .d")!.textContent).toMatch(
      new RegExp(`^${before.uniqueContents} models · `),
    );
  });

  it("once every model is in the vault, say so and offer no plan to review", async () => {
    const h = await home();
    await consolidateAndRescan(h);
    // The sample installs keep a few files no run can move. Leave them out,
    // so the installs hold only links, as the person's do.
    const contents = h.engine.listContents.bind(h.engine);
    vi.spyOn(h.engine, "listContents").mockImplementation(async (req) => {
      const page = await contents(req);
      const rows = page.rows
        .filter((r) => r.inVault)
        .map((r) => ({ ...r, occurrenceCount: r.linkCount }));
      return { ...page, rows, total: rows.length };
    });
    const build = h.engine.buildPlan.bind(h.engine);
    vi.spyOn(h.engine, "buildPlan").mockImplementation(async (scanId) => {
      const plan = await build(scanId);
      return { ...plan, groups: [], totals: { ...plan.totals, bytesFreed: 0, groupsFreeingSpace: 0 } };
    });
    await h.app.actions.refresh();
    await waitFor(() => value("Models still in installs") === "0");

    const vault = await h.engine.listVaultFiles({ offset: 0, limit: 1000 });
    expect(note("Models still in installs")).toBe("every model is in the Library");
    expect(value("Models in the Library")).toBe(String(vault.total));
    expect(document.querySelector(".hero")).toBeNull();
    expect(document.body.textContent).not.toContain("Review the plan");
  });

  it("calls the installs installs, on Home and while a scan runs", async () => {
    const h = await home();
    expect(document.body.textContent).not.toMatch(/instance/i);
    await h.engine.startScan();
    h.engine.devAdvance();
    await waitFor(() => h.app.scanProgress() !== null);
    expect(document.body.textContent).toContain(`${h.app.installs().length} installs · `);
    expect(document.body.textContent).not.toMatch(/instances/i);
  });
});
