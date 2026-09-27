import { screen } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";

import { App } from "~/App";
import { fmt } from "~/domain/format";
import { FixtureEngine } from "~/ipc/fixture/engine";
import { homeShown, renderWithApp, waitFor, type Harness } from "~/test/render";

let harness: Harness | null = null;

afterEach(() => {
  harness?.unmount();
  harness = null;
});

/**
 * Start a run over every group with the machine clear, and hand the test the
 * clock. Nothing moves until the test says so, so how far a run gets is a
 * decision here rather than a race with the machine this runs on.
 */
async function runApply(): Promise<Harness> {
  const engine = new FixtureEngine({ manual: true });
  engine.devSetSymlinksSupported(true);
  engine.devSetComfyRunning(false);
  harness = await renderWithApp(() => <App />, { engine });
  await waitFor(() => harness!.app.plan() !== null);
  harness.app.actions.go("consolidate");
  await waitFor(() => harness!.app.gate().can);
  const apply = screen
    .getAllByRole("button")
    .find((b) => /^Apply/.test(b.textContent ?? ""))!;
  await userEvent.click(apply);
  await waitFor(() => harness!.app.applyProgress() === null);
  return harness;
}

/** Step the run until it has finished exactly this many groups. */
async function upTo(h: Harness, groups: number): Promise<void> {
  for (let step = 0; step < 500; step += 1) {
    if ((h.app.applyProgress()?.groupIndex ?? 0) >= groups) return;
    h.engine.devAdvance();
    await Promise.resolve();
  }
  throw new Error(`the run never reached group ${groups}`);
}

/** Let the run finish, however many steps that takes. */
async function finish(h: Harness): Promise<void> {
  h.engine.devFinish();
  await waitFor(() => h.app.lastApply() !== null);
}

/** The fixture's one file that changes under the run, at group 6. */
const CHANGES_AT = 6;

describe("stopping a run", () => {
  it("adds no failure of its own, because the person asked it to stop", async () => {
    const h = await runApply();
    const { app, engine } = h;
    // Stopped three groups in, before the one file that changes under the run.
    await upTo(h, 3);
    await engine.cancelApply();
    await finish(h);

    const run = app.lastApply()!;
    expect(run.groupsApplied).toBe(3);
    expect(run.groupsApplied).toBeLessThan(CHANGES_AT);
    expect(run.state).toBe("cancelled");
    // The group the stop interrupted was put back, not reported as gone wrong.
    expect(run.failures).toEqual([]);
    expect(run.groupsFailed).toBe(0);
    // It stopped where it was. It did not race to the end of the plan.
    expect(run.groupsApplied).toBeGreaterThan(0);
    // And what it did finish is still there to undo.
    expect(run.revertible).toBe(true);
  });

  it("says the person stopped it, and lists nothing as left alone", async () => {
    const h = await runApply();
    await upTo(h, 3);
    await h.engine.cancelApply();
    await finish(h);

    const text = document.body.textContent ?? "";
    expect(text).toContain("stopped when you asked");
    expect(text).not.toContain("was left alone");
    expect(text).not.toContain("were left alone");
  });

  it("still reports a file that failed before the person pressed stop", async () => {
    const h = await runApply();
    const { app, engine } = h;
    // Stopped after the file that changed, so that failure is already real.
    await upTo(h, CHANGES_AT + 2);
    await engine.cancelApply();
    await finish(h);

    const run = app.lastApply()!;
    expect(run.groupsApplied).toBe(CHANGES_AT + 1);
    expect(run.state).toBe("cancelled");
    // Nothing else in the interface mentions this file, so the stop must not
    // swallow it.
    expect(run.failures).toHaveLength(1);
    expect(run.groupsFailed).toBe(1);
    const text = document.body.textContent ?? "";
    expect(text).toContain("stopped when you asked");
    expect(text).toContain("One file was left alone");
  });

  it("promises only what stopping now does", async () => {
    const h = await runApply();
    await upTo(h, 1);
    const text = document.body.textContent ?? "";
    // The old promise. The engine no longer finishes the file it is on.
    expect(text).not.toContain("Stop after this file");
    expect(text).toContain("Stop now");
    expect(text).toContain("back where it was");
  });
});

describe("a run that cannot be undone any more", () => {
  it("says so before the person confirms, and lists the paths in the way", { timeout: 15000 }, async () => {
    const h = await runApply();
    const { app, engine } = h;
    await finish(h);
    expect(app.lastApply()!.revertible).toBe(true);

    // The person gave a file this run put in the vault another of its names.
    const moved = app.lastApply()!;
    const renamed = (
      await engine.listVaultFiles({ offset: 0, limit: 400 })
    ).files.find((f) => f.addedAt >= moved.startedAt && f.aliases.length > 0)!;
    await engine.setCanonicalName(renamed.sha256, renamed.aliases[0]!);

    await userEvent.click(screen.getByRole("button", { name: /Undo this run/ }));

    // The engine's cost check refuses exactly where the undo would, so the box
    // opens already refused and offers nothing to confirm.
    const dialog = await waitForDialog();
    expect(dialog.textContent).toContain("This run cannot be undone now");
    expect(dialog.textContent).toContain("renamed since");
    expect(screen.queryByRole("button", { name: /Undo the run/ })).toBeNull();
    // The paths, so the person knows what to undo first.
    const paths = dialog.querySelectorAll(".paths li");
    expect(paths.length).toBeGreaterThan(0);
    expect(paths[0]!.textContent).toContain(renamed.aliases[0]!);
    // And the run is still there to undo once they have.
    expect(app.lastApply()!.revertible).toBe(true);
    expect(app.lastApply()!.state).not.toBe("reverted");
  });
});

async function waitForDialog(): Promise<HTMLElement> {
  await waitFor(() => document.querySelector('[role="dialog"] .verdict.no') !== null);
  return document.querySelector('[role="dialog"]') as HTMLElement;
}

describe("the words while a run works and when it is done", () => {
  it("says copies are replaced by links, and space is freed", async () => {
    const h = await runApply();
    await upTo(h, 2);
    const text = (document.body.textContent ?? "").replace(/\s+/g, " ");
    const p = h.app.applyProgress()!;
    expect(text).toContain(`Copies replaced by links${p.linksCreated}`);
    expect(text).toContain(`Space freed${fmt(p.bytesFreed)}`);
    // There were no links before the run, so none are put back.
    expect(text).not.toContain("put back");
    expect(text).not.toContain("returned");
  });

  it("says the space was freed, not that it came back", async () => {
    const h = await runApply();
    await finish(h);
    const run = h.app.lastApply()!;
    const text = (document.body.textContent ?? "").replace(/\s+/g, " ");
    expect(text).toContain("Freed on drive C:.");
    expect(text).toContain(`${fmt(run.bytesFreed)} freed`);
    expect(text).not.toContain("Back on drive");
    expect(text).not.toContain("returned");
  });
});

describe("the numbers on the finished screen", () => {
  it("shows the drive's own before and after, never one derived from the other", async () => {
    const h = await runApply();
    await finish(h);
    const run = h.app.lastApply()!;

    expect(run.vaultFreeBytesBefore).not.toBeNull();
    expect(run.vaultFreeBytesAfter).not.toBeNull();
    const text = document.body.textContent ?? "";
    expect(text).toContain(fmt(run.vaultFreeBytesBefore!));
    expect(text).toContain(fmt(run.vaultFreeBytesAfter!));
    expect(text).toContain("both read from the drive itself");
    // The old line worked the "before" out backwards from the "now".
    expect(text).not.toContain(fmt(run.vaultFreeBytesAfter! - run.bytesFreed));
  });

  it("counts the copies the run removed, not the links it made", async () => {
    const h = await runApply();
    await finish(h);
    const run = h.app.lastApply()!;
    // Every place got a link, the one whose file moved into the vault too, and
    // that file still takes room there.
    expect(run.linksCreated).toBeGreaterThan(run.filesMoved);
    const text = (document.body.textContent ?? "").replace(/\s+/g, " ");
    expect(text).toContain(`${run.linksCreated - run.filesMoved} copies stopped taking room`);
    expect(text).not.toContain(`${run.linksCreated} copies stopped taking room`);
  });

  it("says which number is the run's and which is the drive's", async () => {
    const h = await runApply();
    await finish(h);
    const text = document.body.textContent ?? "";
    expect(text).toContain("had");
    expect(text).toContain("free before and has");
  });

  it("does not count the run's own links as files it could not move", async () => {
    const h = await runApply();
    await finish(h);

    // Measured against the real engine: a plan rebuilt from the scan that ran
    // reports every consolidated path as changed, because the scan recorded a
    // file and the path is a link now. That is the state this test is about,
    // so it fails if the state is not there to test.
    await waitFor(
      () => (h.app.plan()?.blocked ?? []).some((b) => b.reason === "fileChanged"),
      6000,
    );
    const rebuilt = h.app.plan()!;
    expect(
      rebuilt.blocked.filter((b) => b.reason === "fileChanged").length,
    ).toBeGreaterThan(5);

    // The screen reads the plan that ran, which knows what it could not move.
    const ran = h.app.appliedPlan()!;
    expect(ran).not.toBeNull();
    expect(ran.planId).toBe(h.app.lastApply()!.planId);
    expect(ran.blocked.length).toBeLessThan(rebuilt.blocked.length);

    const text = document.body.textContent ?? "";
    expect(text).not.toContain(fmt(rebuilt.totals.blockedBytes));
    // And never more files than the run was even asked about.
    const match = /Still cannot move(\d+) file/.exec(text.replace(/\s+/g, ""));
    if (match) {
      expect(Number(match[1])).toBeLessThanOrEqual(ran.blocked.length);
    }
  });
});

describe("the world after a run, before anything has scanned again", () => {
  it("does not present the old scan's figures as current", async () => {
    const h = await runApply();
    const before = { ...h.app.scan()! };
    await finish(h);
    await waitFor(() => h.app.appliedPlan() !== null, 6000);

    // Measured against the real engine: a run does not scan. The last scan is
    // the same scan, untouched, not one re-recorded as the run finished.
    expect(h.app.scan()!.scanId).toBe(before.scanId);
    expect(h.app.scan()!.finishedAt).toBe(before.finishedAt);
    expect(h.app.scan()!.totals).toEqual(before.totals);
    expect(h.app.scan()!.scanId).toBe(h.app.appliedPlan()!.scanId);
    expect(h.app.scanPredatesRun()).toBe(true);

    h.app.actions.go("home");
    await waitFor(() => homeShown());
    const text = document.body.textContent ?? "";
    expect(text).toContain("not scanned since the run");
    expect(text).toContain("come from the scan taken before the run");
    // And never a claim that the scan is the most recent thing that happened.
    expect(text).not.toContain("last scan just now");
  });

  it("says nothing of the kind once a scan has run since", async () => {
    const h = await runApply();
    await finish(h);
    await waitFor(() => h.app.appliedPlan() !== null, 6000);

    await h.engine.startScan();
    h.engine.devFinish();
    await h.app.actions.refresh();
    await waitFor(() => !h.app.scanPredatesRun(), 6000);

    h.app.actions.go("home");
    const text = document.body.textContent ?? "";
    expect(text).not.toContain("not scanned since the run");
    expect(text).not.toContain("come from the scan taken before the run");
  });
});
