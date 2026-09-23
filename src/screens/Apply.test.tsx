import { screen } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";

import { App } from "~/App";
import { fmt } from "~/domain/format";
import { FixtureEngine } from "~/ipc/fixture/engine";
import { renderWithApp, waitFor, type Harness } from "~/test/render";

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
  it("keeps the modal open and lists the paths that are in the way", { timeout: 15000 }, async () => {
    const h = await runApply();
    const { app, engine } = h;
    await finish(h);
    expect(app.lastApply()!.revertible).toBe(true);

    // The person renamed a file this run put in the vault.
    const moved = app.lastApply()!;
    const renamed = (
      await engine.listVaultFiles({ offset: 0, limit: 400 })
    ).files.find((f) => f.addedAt >= moved.startedAt)!;
    await engine.setCanonicalName(renamed.sha256, "renamed-by-hand.safetensors");

    await userEvent.click(screen.getByRole("button", { name: /Undo this run/ }));
    await userEvent.click(screen.getByRole("button", { name: /Undo the run/ }));

    const dialog = await waitForDialog();
    expect(dialog.textContent).toContain("That did not happen");
    expect(dialog.textContent).toContain("renamed since");
    // The paths, so the person knows what to undo first.
    const paths = dialog.querySelectorAll(".paths li");
    expect(paths.length).toBeGreaterThan(0);
    expect(paths[0]!.textContent).toContain("renamed-by-hand.safetensors");
    // And the run is still there to undo once they have.
    expect(app.lastApply()!.revertible).toBe(true);
    expect(app.lastApply()!.state).not.toBe("reverted");
  });
});

async function waitForDialog(): Promise<HTMLElement> {
  await waitFor(() => document.querySelector('[role="dialog"] .verdict.no') !== null);
  return document.querySelector('[role="dialog"]') as HTMLElement;
}

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

  it("says which number is the run's and which is the drive's", async () => {
    const h = await runApply();
    await finish(h);
    const text = document.body.textContent ?? "";
    expect(text).toContain("had");
    expect(text).toContain("free before and has");
  });

  it("does not call a finished file one that cannot move", async () => {
    const h = await runApply();
    await finish(h);
    // After a run every consolidated path reads as already in the vault, and
    // the engine reports each one as a blocked row. That is the state this
    // test is about, so it fails if the state is not there to test.
    await waitFor(() =>
      (h.app.plan()?.blocked ?? []).some((b) => b.reason === "alreadyInVault"),
      6000,
    );
    const raw = h.app.plan()!;
    const finished = raw.blocked.filter((b) => b.reason === "alreadyInVault");
    expect(finished.length).toBeGreaterThan(5);

    // None of them reaches the person as a file that could not move.
    const view = h.app.planView()!;
    expect(view.blocked.some((b) => b.reason === "alreadyInVault")).toBe(false);
    expect(view.blocked.length).toBeLessThan(raw.blocked.length);

    // The count and the size on screen come from the same set of rows. They
    // used to come from two, which is how 9 of 9 done sat above 18 files stuck.
    expect(view.blockedBytes).toBe(
      view.blocked.reduce((sum, row) => sum + row.sizeBytes, 0),
    );
    expect(view.blockedBytes).toBeLessThan(raw.totals.blockedBytes);

    const text = document.body.textContent ?? "";
    expect(text).not.toContain(fmt(raw.totals.blockedBytes));
    if (view.blocked.length === 0) {
      expect(text).not.toContain("Still cannot move");
    }
  });
});
