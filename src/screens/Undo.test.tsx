import { screen, within } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";

import { App } from "~/App";
import { fmt } from "~/domain/format";
import { FixtureEngine } from "~/ipc/fixture/engine";
import { costLine } from "~/modals/undo";
import { renderWithApp, waitFor, type Harness } from "~/test/render";

let harness: Harness | null = null;

afterEach(() => {
  harness?.unmount();
  harness = null;
});

/**
 * A finished run over every group, then the undo started and not advanced.
 * The test holds the clock, so how far the undo got is its decision.
 */
async function startUndo(): Promise<Harness> {
  const engine = new FixtureEngine({ manual: true });
  engine.devSetSymlinksSupported(true);
  engine.devSetComfyRunning(false);
  const h = await renderWithApp(() => <App />, { engine });
  harness = h;
  await waitFor(() => h.app.plan() !== null);
  h.app.actions.go("consolidate");
  await waitFor(() => h.app.gate().can);
  const apply = screen
    .getAllByRole("button")
    .find((b) => /^Apply/.test(b.textContent ?? ""))!;
  await userEvent.click(apply);
  engine.devFinish();
  await waitFor(() => h.app.lastApply() !== null && h.app.applyProgress() === null);

  await userEvent.click(screen.getByRole("button", { name: /Undo this run/ }));
  await userEvent.click(screen.getByRole("button", { name: /Undo the run/ }));
  await waitFor(() => document.querySelector('[role="dialog"]') === null);
  return h;
}

/** Step the undo until the screen has a progress report to show. */
async function advance(h: Harness, steps: number): Promise<void> {
  h.engine.devAdvance(steps);
  await waitFor(() => h.app.revertProgress() !== null);
}

const text = () => (document.body.textContent ?? "").replace(/\s+/g, " ");

describe("the screen while a run is undone", () => {
  it("says it is undoing, not applying", async () => {
    const h = await startUndo();
    await advance(h, 1);

    expect(text()).toContain("Undoing");
    expect(text()).toContain("Putting files back");
    expect(text()).not.toContain("Applying");
    expect(text()).not.toContain("Moving and linking");
    expect(text()).not.toContain("renamed rather than copied");
    // Stopping an undo half way leaves a tree that is neither the run nor the
    // world before it, and nothing describes that state, so it is not offered.
    expect(screen.queryByRole("button", { name: /Stop now/ })).toBeNull();
    // The rail beside it says the same thing, not what the run freed.
    expect(text()).toContain("putting files back");
    expect(text()).not.toContain("freed just now");
  });

  it("reports what the undo has put back as it goes", async () => {
    const h = await startUndo();
    await advance(h, 60);
    const p = h.app.revertProgress()!;
    expect(p.filesPutBack).toBeGreaterThan(0);
    expect(p.linksRemoved).toBeGreaterThan(0);
    expect(p.bytesCopied).toBeGreaterThan(0);
    expect(p.bytesCopied).toBeLessThan(p.bytesToCopy);

    expect(text()).toContain(`Files back where they were${p.filesPutBack} files`);
    expect(text()).toContain(`Links removed${p.linksRemoved} places`);
    expect(text()).toContain(`Copied back out of the vault${fmt(p.bytesCopied)}`);
    expect(text()).toContain(`${fmt(p.bytesCopied)} of ${fmt(p.bytesToCopy)} copied back`);
    expect(text()).toContain(`${p.filesPutBack} of ${p.filesToPutBack} files back in place`);
  });

  it("never shows the engine's journal steps as a count", async () => {
    const h = await startUndo();
    await advance(h, 60);
    const p = h.app.revertProgress()!;
    // A model is several steps. "44 of 54 files" was steps, and 54 matched
    // nothing the person had seen.
    expect(p.stepTotal).not.toBe(p.filesToPutBack);
    expect(text()).not.toContain(`of ${p.stepTotal}`);
  });

  it("follows the bytes copied back, not the steps, for the bar", async () => {
    const h = await startUndo();
    await advance(h, 30);
    const p = h.app.revertProgress()!;
    const byBytes = Math.round((p.bytesCopied / p.bytesToCopy) * 100);
    const bySteps = Math.round((p.stepIndex / p.stepTotal) * 100);
    expect(byBytes).not.toBe(bySteps);
    expect(document.querySelector(".pct")!.textContent).toBe(`${byBytes}%`);
  });

  it("leaves the undo screen when the undo fails", async () => {
    const h = await startUndo();
    await advance(h, 1);
    h.engine.devFailRevert("There is not enough room on that drive to put the files back.");
    await waitFor(() => h.app.revertProgress() === null);
    expect(text()).not.toContain("Putting files back");
    expect(text()).toContain("There is not enough room on that drive");
  });
});

describe("the plan after an undo, before anything has scanned again", () => {
  it("does not show a plan built from the scan the undo made stale", async () => {
    const h = await startUndo();
    const scanned = h.app.scan()!;
    h.engine.devFinish();
    await waitFor(() => h.app.lastApply() === null && h.app.revertProgress() === null);
    await waitFor(() => h.app.scanPredatesUndo());

    // Measured against the real engine: an undo does not scan.
    expect(h.app.scan()!.scanId).toBe(scanned.scanId);
    expect(text()).toContain("Your installs changed since the last scan");
    expect(text()).not.toContain("The plan");
    expect(text()).not.toContain("copies become links");
    expect(screen.getByRole("button", { name: /Scan now/ })).toBeTruthy();

    // Nothing is built from that scan, so no screen can print it: not the
    // amount to reclaim, and not a claim that every model is held once.
    expect(h.app.plan()).toBeNull();
    expect(text()).not.toContain("can be freed");
    expect(text()).not.toContain("held once");

    h.app.actions.go("home");
    await waitFor(() => text().includes("Instances"));
    expect(text()).not.toContain("held twice or more");
    expect(text()).not.toContain("held once");
    expect(text()).toContain("not scanned since the undo");
    expect(text()).toContain("taken before the undo put the files back");
  });

  it("shows the plan again once a scan has run since", async () => {
    const h = await startUndo();
    h.engine.devFinish();
    await waitFor(() => h.app.scanPredatesUndo());

    await h.engine.startScan();
    h.engine.devFinish();
    await h.app.actions.refresh();
    await waitFor(() => !h.app.scanPredatesUndo());

    expect(text()).not.toContain("Your installs changed since the last scan");
    expect(text()).toContain("The plan");
  });
});

/** A finished run, with the Undo box opened on it. */
async function openUndoBox(prepare?: (engine: FixtureEngine) => void): Promise<Harness> {
  const engine = new FixtureEngine({ manual: true });
  engine.devSetSymlinksSupported(true);
  engine.devSetComfyRunning(false);
  prepare?.(engine);
  const h = await renderWithApp(() => <App />, { engine });
  harness = h;
  await waitFor(() => h.app.plan() !== null);
  h.app.actions.go("consolidate");
  await waitFor(() => h.app.gate().can);
  await userEvent.click(
    screen.getAllByRole("button").find((b) => /^Apply/.test(b.textContent ?? ""))!,
  );
  engine.devFinish();
  await waitFor(() => h.app.lastApply() !== null && h.app.applyProgress() === null);
  await userEvent.click(screen.getByRole("button", { name: /Undo this run/ }));
  await waitFor(() => document.querySelector('[role="dialog"]') !== null);
  return h;
}

const dialogText = () =>
  (document.querySelector('[role="dialog"]')?.textContent ?? "").replace(/\s+/g, " ");

describe("the Undo box", () => {
  it("states the engine's own cost check, not the run's sizes", async () => {
    // Sparse models: their copies take a sliver of their size.
    const room = 196_608;
    const h = await openUndoBox((e) => e.devSetRevertRoom(room));
    const preview = await h.engine.previewRevert(h.app.lastApply()!.applyId);
    expect(preview.filesCopiedBack).toBeGreaterThan(0);

    const text = dialogText();
    expect(text).toContain(`${preview.filesRenamedBack} files come back at once, by a rename`);
    expect(text).toContain(`${preview.filesCopiedBack} files have to be copied back out of the vault`);
    expect(text).toContain(`${fmt(preview.bytesToCopy)} in all`);
    expect(text).toContain("The copying is what takes the time");
    expect(text).toContain("Drive C: is expected to need less than 1 MB for the copies");
    expect(text).toContain(`and has ${fmt(preview.drives[0]!.freeBytes!)} free.`);
    // The old line: the room the drive "takes back", from nominal size.
    expect(text).not.toContain("takes back");
    expect(screen.getByRole("button", { name: /Undo the run/ })).toBeTruthy();
  });

  it("offers no confirm when a drive is short of room, and says which", async () => {
    await openUndoBox((e) => e.devSetRevertRoom(10 * 1024 ** 4));
    const text = dialogText();
    expect(text).toContain("That is not enough.");
    expect(text).toContain("There is not enough room to undo this run");
    // The run put copies back on two drives, and both are named.
    expect(text).toContain("Free some space on drive C: and D:");
    expect(screen.queryByRole("button", { name: /Undo the run/ })).toBeNull();
    const dialog = document.querySelector('[role="dialog"]') as HTMLElement;
    expect(within(dialog).getByRole("button", { name: /^Close$/ })).toBeTruthy();
  });

  it("does not claim room it could not read", async () => {
    await openUndoBox((e) => e.devSetDriveReadable(false));
    const text = dialogText();
    expect(text).toContain("It did not answer when asked how much room it has.");
    expect(text).not.toContain("0 B free");
    expect(screen.getByRole("button", { name: /Undo the run/ })).toBeTruthy();
  });

  it("says nothing comes back by a rename when every kept copy is on another drive", () => {
    // A vault on another drive than the installs: every kept copy is copied
    // back as well, so nothing is renamed. The double keeps its vault on the
    // installs' drive, so this is read from the engine's sample shape directly.
    const line = costLine({
      applyId: "apply-1",
      filesRenamedBack: 0,
      filesCopiedBack: 3,
      bytesToCopy: 13_876_297_728,
      drives: [],
    })
      .map((part) => part.text)
      .join("");
    expect(line).not.toContain("0 files come back at once");
    expect(line).not.toContain("by a rename");
    expect(line).toContain("3 files have to be copied back out of the vault");
  });
});
