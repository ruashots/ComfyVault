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

describe("Home's count of the models in the installs", () => {
  it("counts every model found by the scan before any run", async () => {
    const h = await home();
    const t = h.app.scan()!.totals;
    expect(value("Installs")).toBe(String(h.app.installs().length));
    expect(value("Unique models")).toBe(String(t.uniqueContents));
    expect(note("Unique models")).toBe(`${t.movableFiles} files on disk`);
    expect(value("Models on disk")).toBe(fmt(t.movableBytes).replace(" ", ""));
    expect(note("Models on disk")).toBe(`${fmt(t.uniqueBytes)} if kept once`);
  });

  it("still counts every model once a run put them in the vault", async () => {
    const h = await home();
    const before = h.app.scan()!.totals;
    await consolidateAndRescan(h);
    const after = h.app.scan()!.totals;
    // The new scan counts only what is left to consolidate. The rest is
    // still in the installs, reached through links.
    expect(after.movableFiles).toBeLessThan(before.movableFiles / 10);

    expect(value("Unique models")).toBe(String(before.uniqueContents));
    expect(note("Unique models")).toBe(
      `${after.movableFiles + after.alreadyLinkedFiles} files in the installs, ${after.alreadyLinkedFiles} of them links to the vault`,
    );
    // Each vault file takes its space once, plus what is still out.
    const onDisk =
      h.app
        .vaultFiles()
        .filter((f) => f.linkCount > 0)
        .reduce((sum, f) => sum + f.sizeBytes, 0) + after.movableBytes;
    expect(value("Models on disk")).toBe(fmt(onDisk).replace(" ", ""));
    expect(document.body.textContent).toContain(
      `${fmt(onDisk)} of models across ${h.app.installs().length} installs`,
    );
    expect(document.querySelector(".act .d")!.textContent).toMatch(
      new RegExp(`^${before.uniqueContents} models · `),
    );
    for (const row of document.querySelectorAll(".inst")) {
      expect(Number(row.querySelector(".c1")!.textContent)).toBeGreaterThan(after.movableFiles);
    }
  });

  it("says every model is in the vault when nothing is left to move", async () => {
    const engine = new FixtureEngine({ manual: true });
    engine.devSetSymlinksSupported(true);
    engine.devSetComfyRunning(false);
    // The sample installs hold a few files no run can move, so this stands in
    // for installs where every model went into the vault.
    const contents = engine.listContents.bind(engine);
    vi.spyOn(engine, "listContents").mockImplementation(async (req) => {
      const page = await contents(req);
      return { ...page, rows: page.rows.map((r) => ({ ...r, linkCount: r.occurrenceCount })) };
    });
    const build = engine.buildPlan.bind(engine);
    vi.spyOn(engine, "buildPlan").mockImplementation(async (scanId) => {
      const plan = await build(scanId);
      return {
        ...plan,
        groups: [],
        totals: { ...plan.totals, bytesFreed: 0, groupsFreeingSpace: 0 },
      };
    });
    harness = await renderWithApp(() => <App />, { engine });
    await waitFor(tilesShown);

    expect(hero()).toContain("Every model in your installs is in the vault.");
    expect(hero()).toContain("Nothing is left to move, so there is no space to free.");
    expect(hero()).not.toContain("Consolidating moves them into the vault");
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
