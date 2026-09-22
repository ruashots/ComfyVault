import { screen } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";

import { fmt } from "~/domain/format";
import { FixtureEngine } from "~/ipc/fixture/engine";
import { ConsolidateScreen } from "~/screens/Consolidate";
import { renderWithApp, waitFor, type Harness } from "~/test/render";

let harness: Harness | null = null;

afterEach(() => {
  harness?.unmount();
  harness = null;
});

async function mount(prepare?: (engine: FixtureEngine) => void) {
  const engine = new FixtureEngine();
  prepare?.(engine);
  harness = await renderWithApp(() => <ConsolidateScreen />, { engine });
  return harness;
}

const applyButton = () =>
  screen
    .getAllByRole("button")
    .find((b) => /apply|nothing is ticked/i.test(b.textContent ?? ""))!;

describe("Apply is never pressable while the engine says it is blocked", () => {
  it("is disabled and names how many things to fix", async () => {
    await mount();
    const button = applyButton();
    expect(button).toBeDisabled();
    expect(button.textContent).toContain("Apply blocked");
    expect(button.textContent).toContain("2 to fix");
  });

  it("still refuses when only Developer Mode is off", async () => {
    const { app } = await mount((engine) => {
      engine.devSetComfyRunning(false);
    });
    await waitFor(() => app.machine()?.running.length === 0);
    const button = applyButton();
    expect(button).toBeDisabled();
    expect(button.textContent).toContain("1 to fix");
  });

  it("still refuses when only a ComfyUI is running", async () => {
    const { app } = await mount((engine) => {
      engine.devSetDeveloperMode(true);
    });
    await waitFor(() => app.machine()?.developerMode === true);
    const button = applyButton();
    expect(button).toBeDisabled();
    expect(button.textContent).toContain("1 to fix");
  });

  it("becomes pressable once the machine is clear", async () => {
    const { app } = await mount((engine) => {
      engine.devSetDeveloperMode(true);
      engine.devSetComfyRunning(false);
    });
    await waitFor(() => app.gate().can);
    const button = applyButton();
    expect(button).toBeEnabled();
    expect(button.textContent).toContain("Apply this plan");
  });

  it("refuses again when the person unticks everything", async () => {
    const { app } = await mount((engine) => {
      engine.devSetDeveloperMode(true);
      engine.devSetComfyRunning(false);
    });
    await waitFor(() => app.gate().can);
    for (const model of app.plan()!.models) app.actions.toggleModel(model.id);
    await waitFor(() => app.selection().moves === 0);
    const button = applyButton();
    expect(button).toBeDisabled();
    expect(button.textContent).toContain("Nothing is ticked");
  });

  it("does not start a run while it is blocked, even if the button is clicked", async () => {
    const { app } = await mount();
    let started = false;
    const original = app.engine.startApply.bind(app.engine);
    app.engine.startApply = async (ids) => {
      started = true;
      return original(ids);
    };
    await userEvent.click(applyButton());
    expect(started).toBe(false);
    expect(app.applyProgress()).toBeNull();
  });
});

describe("the commit bar moves as rows are ticked and unticked", () => {
  it("shows the whole plan when nothing is unticked", async () => {
    const { app } = await mount();
    const selection = app.selection();
    const bar = document.querySelector(".commit")!;
    expect(bar.textContent).toContain(`${selection.moves} files move`);
    expect(bar.textContent).toContain(`${selection.links} links go back`);
    expect(bar.textContent).toContain(fmt(selection.bytes));
  });

  it("takes a group's win out of the figure when its row is unticked", async () => {
    const { app } = await mount();
    const biggest = app.plan()!.duplicates[0]!;
    const before = app.selection();

    const row = screen.getByRole("button", {
      name: `Include ${biggest.filename}`,
    });
    await userEvent.click(row);
    await waitFor(() => app.selection().moves === before.moves - 1);

    const after = app.selection();
    expect(after.bytes).toBe(before.bytes - biggest.reclaimBytes);
    expect(after.links).toBe(before.links - biggest.live.length);
    const bar = document.querySelector(".commit")!;
    expect(bar.textContent).toContain(fmt(after.bytes));
    expect(bar.textContent).toContain(`${after.moves} files move`);
  });

  it("puts the figure back when the row is ticked again", async () => {
    const { app } = await mount();
    const biggest = app.plan()!.duplicates[0]!;
    const before = app.selection().bytes;
    const row = screen.getByRole("button", {
      name: `Include ${biggest.filename}`,
    });
    await userEvent.click(row);
    await waitFor(() => app.selection().bytes !== before);
    await userEvent.click(row);
    await waitFor(() => app.selection().bytes === before);
    expect(document.querySelector(".commit")!.textContent).toContain(fmt(before));
  });
});

describe("the report says what is holding Apply back", () => {
  it("names both reasons and what closing ComfyUI is worth", async () => {
    const { app } = await mount();
    const panel = document.querySelector(".blk")!;
    expect(panel.textContent).toContain("Windows Developer Mode is off");
    expect(panel.textContent).toContain("ComfyUI-Alpha is running");
    expect(panel.textContent).toContain("pid 18244");
    expect(panel.textContent).toContain(fmt(app.plan()!.totals.reclaimBytes));
    expect(panel.textContent).toContain(fmt(app.reclaimIfClosed()));
  });

  it("lists every copy that cannot move, each with its reason", async () => {
    const { app } = await mount();
    expect(app.plan()!.blocked).toHaveLength(8);
    const text = document.body.textContent ?? "";
    expect(text).toContain(
      "ComfyUI-Alpha is running and has this file open.",
    );
    expect(text).toContain(
      "This copy sits on drive D:. The vault is on drive C:.",
    );
    expect(text).toContain(
      "Windows refused to move this file. ComfyVault does not have write permission",
    );
    // No raw token from the engine ever reaches the glass.
    expect(text).not.toContain("file_open");
    expect(text).not.toContain("other_drive");
    expect(text).not.toContain("permission_denied");
  });

  it("says in the group itself when one copy of a duplicate is held open", async () => {
    await mount();
    const text = document.body.textContent ?? "";
    expect(text).toContain("the file is open");
    expect(text).toContain("one copy could not be read");
  });

  it("offers to copy across only for a copy on another drive", async () => {
    await mount();
    const copyButtons = screen.getAllByRole("button", {
      name: "Copy it instead",
    });
    const recheckButtons = screen.getAllByRole("button", { name: "Re-check" });
    expect(copyButtons).toHaveLength(4);
    expect(recheckButtons).toHaveLength(4);
  });
});
