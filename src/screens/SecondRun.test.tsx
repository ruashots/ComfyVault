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
const applyButton = () =>
  screen.getAllByRole("button").find((b) => /^Apply/.test(b.textContent ?? ""))!;

describe("a second run after a finished one", () => {
  it("plans and applies what a later scan found, without undoing the first", async () => {
    const engine = new FixtureEngine({ manual: true });
    engine.devSetSymlinksSupported(true);
    engine.devSetComfyRunning(false);
    const h = await renderWithApp(() => <App />, { engine });
    harness = h;
    await waitFor(() => h.app.plan() !== null);
    h.app.actions.go("consolidate");
    await waitFor(() => h.app.gate().can);

    // The first run leaves two models for later, the way a model downloaded
    // after it would be waiting.
    const later = h.app.planView()!.duplicates.slice(0, 2).map((g) => g.groupId);
    for (const id of later) h.app.actions.toggleGroup(id);
    await userEvent.click(applyButton());
    engine.devFinish();
    await waitFor(() => h.app.runOnScreen() !== null && h.app.applyProgress() === null);
    const first = h.app.lastApply()!;
    expect(text()).toContain("Undo this run");

    await engine.startScan();
    engine.devFinish();
    await h.app.actions.refresh();

    // The new scan's plan replaces the finished screen.
    await waitFor(() => h.app.runOnScreen() === null);
    await waitFor(() => text().includes("The plan"));
    const planned = h.app.plan()!.groups.map((g) => g.groupId);
    for (const id of later) expect(planned).toContain(id);
    expect(text()).toContain("Undo the last run");

    await waitFor(() => h.app.gate().can);
    await userEvent.click(applyButton());
    engine.devFinish();
    await waitFor(() => h.app.lastApply()?.applyId !== first.applyId && h.app.applyProgress() === null);

    const second = h.app.lastApply()!;
    expect(second.state).toMatch(/^completed/);
    expect(second.groupIds).toEqual(expect.arrayContaining(later));
    expect((await engine.listApplies()).length).toBe(2);
    expect(text()).toContain("Undo this run");
  });
});
