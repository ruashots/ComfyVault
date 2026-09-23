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

/** Start a run over every group, with the machine clear and the clock fast. */
async function runApply(speed = 200): Promise<Harness> {
  const engine = new FixtureEngine({ speed });
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
  return harness;
}

/** The fixture's one file that changes under the run, at group 6. */
const CHANGES_AT = 6;

describe("stopping a run", () => {
  it("adds no failure of its own, because the person asked it to stop", async () => {
    // Slow enough to stop before the one file that changes under the run.
    const { app, engine } = await runApply(20);
    await waitFor(() => app.applyProgress() !== null);
    await engine.cancelApply();
    await waitFor(() => app.lastApply() !== null);

    const run = app.lastApply()!;
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
    const { app, engine } = await runApply(20);
    await waitFor(() => app.applyProgress() !== null);
    await engine.cancelApply();
    await waitFor(() => app.lastApply() !== null);

    const text = document.body.textContent ?? "";
    expect(text).toContain("stopped when you asked");
    expect(text).not.toContain("was left alone");
    expect(text).not.toContain("were left alone");
  });

  it("still reports a file that failed before the person pressed stop", async () => {
    const { app, engine } = await runApply();
    await waitFor(() => (app.applyProgress()?.groupIndex ?? 0) > CHANGES_AT);
    await engine.cancelApply();
    await waitFor(() => app.lastApply() !== null);

    const run = app.lastApply()!;
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
    const { app } = await runApply();
    await waitFor(() => app.applyProgress() !== null);
    const text = document.body.textContent ?? "";
    // The old promise. The engine no longer finishes the file it is on.
    expect(text).not.toContain("Stop after this file");
    expect(text).toContain("Stop now");
    expect(text).toContain("back where it was");
  });
});

describe("a run that cannot be undone any more", () => {
  it("keeps the modal open and lists the paths that are in the way", { timeout: 15000 }, async () => {
    const { app, engine } = await runApply();
    await waitFor(() => app.lastApply() !== null, 8000);
    await waitFor(() => app.lastApply()!.revertible);

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
