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

const text = () => (document.body.textContent ?? "").replace(/\s+/g, " ");

/**
 * A run over every group, cut off three groups in the way a crash ends it, and
 * the app opened again on the same engine, the way it opens after a restart.
 */
async function cutOffAndReopen(): Promise<Harness> {
  const engine = new FixtureEngine({ manual: true });
  engine.devSetSymlinksSupported(true);
  engine.devSetComfyRunning(false);
  const first = await renderWithApp(() => <App />, { engine });
  await waitFor(() => first.app.plan() !== null);
  first.app.actions.go("consolidate");
  await waitFor(() => first.app.gate().can);
  await userEvent.click(
    screen.getAllByRole("button").find((b) => /^Apply/.test(b.textContent ?? ""))!,
  );
  for (let i = 0; i < 500 && (first.app.applyProgress()?.groupIndex ?? 0) < 3; i += 1) {
    engine.devAdvance();
    await Promise.resolve();
  }
  engine.devCutOffApply();
  first.unmount();

  const h = await renderWithApp(() => <App />, { engine });
  harness = h;
  h.app.actions.go("consolidate");
  await waitFor(() => h.app.cutOffRun() !== null);
  return h;
}

describe("a run cut off part way", () => {
  it("offers to finish it or undo it, not a finished screen", async () => {
    const h = await cutOffAndReopen();
    expect(h.app.lastApply()!.state).toBe("running");

    expect(text()).toContain("A run stopped part way through");
    expect(screen.getByRole("button", { name: /Finish it/ })).toBeTruthy();
    expect(screen.getByRole("button", { name: /Undo it/ })).toBeTruthy();
    // What the finished screen printed for a state it had no words for.
    expect(text()).not.toContain("undefined");
    expect(text()).not.toContain("copies stopped taking room");
  });

  it("finishes it, and the finished screen then reads the whole run", async () => {
    const h = await cutOffAndReopen();
    const before = h.app.lastApply()!;
    expect(before.groupsApplied).toBeLessThan(before.groupsRequested);

    await userEvent.click(screen.getByRole("button", { name: /Finish it/ }));
    await waitFor(() => h.app.applyProgress() !== null || h.app.cutOffRun() === null);
    h.engine.devFinish();
    await waitFor(() => h.app.lastApply()?.state === "completed");

    const run = h.app.lastApply()!;
    // One run, described whole, as the contract says a resumed run is.
    expect(run.groupsApplied).toBe(run.groupsRequested);
    expect(text()).toContain("finished ·");
    expect(text()).toContain(`${run.groupsApplied} of ${run.groupsRequested} done`);
    expect(text()).not.toContain("undefined");
    expect(text()).not.toContain("A run stopped part way through");
  });

  it("undoes it through the same box", async () => {
    await cutOffAndReopen();
    await userEvent.click(screen.getByRole("button", { name: /Undo it/ }));
    await waitFor(() => document.querySelector('[role="dialog"]') !== null);
    const dialog = document.querySelector('[role="dialog"]')!.textContent ?? "";
    expect(dialog).toContain("Undo this run");
    expect(dialog).toContain("come back at once, by a rename");
  });

  it("tells Home and the rail, instead of what the run freed", async () => {
    const h = await cutOffAndReopen();
    h.app.actions.go("home");
    await waitFor(() => text().includes("Instances"));
    expect(text()).toContain("A run stopped part way through.");
    expect(text()).toContain("a run stopped part way");
    expect(text()).not.toContain("freed just now");
    expect(text()).not.toContain("copies are now links");
  });
});

describe("a cut-off run the engine will not touch", () => {
  async function blockedAndReopen(): Promise<Harness> {
    const h = await cutOffAndReopen();
    h.engine.devBlockCutOffRun([
      "C:\\ComfyUI-Old\\models\\checkpoints\\a.safetensors",
      "C:\\ComfyUI-Old\\models\\checkpoints\\b.safetensors",
      "C:\\ComfyUI-Old\\models\\loras\\c.safetensors",
      "C:\\ComfyUI-Old\\models\\loras\\d.safetensors",
    ]);
    await h.app.actions.refresh();
    await waitFor(() => h.app.cutOffRun()?.blocked === true);
    return h;
  }

  it("offers only to set it aside, and names the places in the way", async () => {
    await blockedAndReopen();
    expect(text()).toContain("ComfyVault cannot finish or undo it");
    expect(text()).toContain("C:\\ComfyUI-Old\\models\\checkpoints\\a.safetensors");
    expect(text()).toContain("and 1 more");
    expect(screen.queryByRole("button", { name: /Finish it/ })).toBeNull();
    expect(screen.queryByRole("button", { name: /Undo it/ })).toBeNull();
    expect(text()).toContain("A vault someone else prepared can cause it too");
  });

  it("confirms with every fact the person needs, then leaves the list of runs to settle", async () => {
    const h = await blockedAndReopen();
    await userEvent.click(screen.getByRole("button", { name: /^Set it aside$/ }));
    await waitFor(() => document.querySelector('[role="dialog"]') !== null);
    const dialog = (document.querySelector('[role="dialog"]')!.textContent ?? "").replace(/\s+/g, " ");
    expect(dialog).toContain("Setting it aside moves nothing on the disk.");
    expect(dialog).toContain("Every link this run made keeps pointing into the vault, so every model keeps loading.");
    expect(dialog).toContain("the run can no longer be undone from ComfyVault");
    expect(dialog).toContain("the vault being opened on a different computer, or an install moved or removed after the run");
    expect(dialog).toContain("A vault someone else prepared can cause it too.");
    // Every place, not the first three.
    for (const name of ["a", "b", "c", "d"]) expect(dialog).toContain(`${name}.safetensors`);

    const inDialog = document.querySelector('[role="dialog"]') as HTMLElement;
    await userEvent.click(
      [...inDialog.querySelectorAll("button")].find((b) => b.textContent === "Set it aside")!,
    );
    await waitFor(() => h.app.lastApply()?.state === "setAside");
    expect(h.app.cutOffRun()).toBeNull();
    expect((await h.engine.getInterruptedApplies()).length).toBe(0);
    // The last scan predates the run, so it is not shown as a plan.
    await waitFor(() => text().includes("Your installs changed since the last scan"));
    expect(text()).toContain("The run you set aside");
    expect(text()).not.toContain("undefined");
  });
});
