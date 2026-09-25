import { screen } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";

import { fmt } from "~/domain/format";
import { fileNameOf } from "~/domain/view";
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
  await waitFor(() => harness!.app.plan() !== null);
  return harness;
}

const clearMachine = (engine: FixtureEngine) => {
  engine.devSetSymlinksSupported(true);
  engine.devSetComfyRunning(false);
};

const applyButton = () =>
  screen
    .getAllByRole("button")
    .find((b) => /apply|nothing is ticked/i.test(b.textContent ?? ""))!;

describe("Apply is never pressable while the engine says it is blocked", () => {
  it("is disabled and says how many things to fix", async () => {
    await mount();
    const button = applyButton();
    expect(button).toBeDisabled();
    expect(button.textContent).toContain("Apply blocked");
    expect(button.textContent).toContain("2 to fix");
  });

  it("still refuses when only links are unavailable", async () => {
    const { app } = await mount((engine) => engine.devSetComfyRunning(false));
    await waitFor(() => app.running().length === 0);
    const button = applyButton();
    expect(button).toBeDisabled();
    expect(button.textContent).toContain("1 to fix");
  });

  it("still refuses when only a ComfyUI is running", async () => {
    const { app } = await mount((engine) => engine.devSetSymlinksSupported(true));
    await waitFor(() => app.appState()?.platform.symlinks.supported === true);
    const button = applyButton();
    expect(button).toBeDisabled();
    expect(button.textContent).toContain("1 to fix");
  });

  it("becomes pressable once the machine is clear", async () => {
    const { app } = await mount(clearMachine);
    await waitFor(() => app.gate().can);
    const button = applyButton();
    expect(button).toBeEnabled();
    expect(button.textContent).toContain("Apply this plan");
  });

  it("refuses again when the person unticks everything", async () => {
    const { app } = await mount(clearMachine);
    await waitFor(() => app.gate().can);
    for (const group of app.plan()!.groups) app.actions.toggleGroup(group.groupId);
    await waitFor(() => app.selection().moves === 0);
    const button = applyButton();
    expect(button).toBeDisabled();
    expect(button.textContent).toContain("Nothing is ticked");
  });

  it("does not start a run while it is blocked, even if the button is clicked", async () => {
    const { app } = await mount();
    let started = false;
    const original = app.engine.startApply.bind(app.engine);
    app.engine.startApply = async (args) => {
      started = true;
      return original(args);
    };
    await userEvent.click(applyButton());
    expect(started).toBe(false);
    expect(app.applyProgress()).toBeNull();
  });

  it("sends only the ticked groups when it does run", async () => {
    const { app } = await mount(clearMachine);
    await waitFor(() => app.gate().can);
    const dropped = app.plan()!.groups[0]!.groupId;
    app.actions.toggleGroup(dropped);
    await waitFor(() => app.selection().groupIds.length === app.plan()!.groups.length - 1);

    let sent: string[] = [];
    app.engine.startApply = async (args) => {
      sent = args.groupIds;
      return { applyId: "apply-test" };
    };
    await userEvent.click(applyButton());
    await waitFor(() => sent.length > 0);
    expect(sent).not.toContain(dropped);
    expect(sent).toHaveLength(app.plan()!.groups.length - 1);
  });
});

describe("the commit bar moves as rows are ticked and unticked", () => {
  it("shows the whole plan when nothing is unticked", async () => {
    const { app } = await mount(clearMachine);
    const selection = app.selection();
    const bar = document.querySelector(".commit")!;
    expect(bar.textContent).toContain(`${selection.moves} files move`);
    expect(bar.textContent).toContain(`${selection.links} links go back`);
    expect(bar.textContent).toContain(fmt(selection.bytes));
  });

  it("takes a group's win out of the figure when its row is unticked", async () => {
    const { app } = await mount(clearMachine);
    const biggest = app.planView()!.duplicates[0]!;
    const before = app.selection();

    const row = screen.getByRole("button", {
      name: `Include ${fileNameOf(biggest.vaultRelPath)}`,
    });
    await userEvent.click(row);
    await waitFor(() => app.selection().moves === before.moves - 1);

    const after = app.selection();
    expect(after.bytes).toBe(before.bytes - biggest.bytesFreed);
    expect(after.links).toBe(before.links - biggest.occurrences);
    expect(document.querySelector(".commit")!.textContent).toContain(fmt(after.bytes));
  });

  it("puts the figure back when the row is ticked again", async () => {
    const { app } = await mount(clearMachine);
    const biggest = app.planView()!.duplicates[0]!;
    const before = app.selection().bytes;
    const row = screen.getByRole("button", {
      name: `Include ${fileNameOf(biggest.vaultRelPath)}`,
    });
    await userEvent.click(row);
    await waitFor(() => app.selection().bytes !== before);
    await userEvent.click(row);
    await waitFor(() => app.selection().bytes === before);
    expect(document.querySelector(".commit")!.textContent).toContain(fmt(before));
  });
});

describe("the report says what is holding Apply back", () => {
  it("shows the engine's own guidance word for word", async () => {
    await mount();
    const panel = document.querySelector(".blk")!;
    expect(panel.textContent).toContain(
      "Windows needs Developer Mode to create the links this app uses.",
    );
    expect(panel.textContent).toContain("os error 1314");
    expect(panel.textContent).toContain("is running");
    expect(panel.textContent).toContain("pid 18244");
  });

  it("explains every file it will not move, in words", async () => {
    const { app } = await mount((engine) => engine.devSetSymlinksSupported(true));
    await waitFor(() => app.appState()?.platform.symlinks.supported === true);
    const text = document.body.textContent ?? "";
    expect(app.planView()!.blocked.length).toBeGreaterThan(0);
    expect(text).toContain("has this file open");
    expect(text).toContain("Windows refused to touch this file");
    // No token from the engine ever reaches the glass.
    for (const token of [
      "fileLocked",
      "permissionDenied",
      "symlinkUnsupported",
      "inCustomNodes",
      "sameVolume",
      "firstByPath",
    ]) {
      expect(text, token).not.toContain(token);
    }
  });

  it("names the yaml category when a folder name would escape the vault", async () => {
    const { app } = await mount((engine) => engine.devSetSymlinksSupported(true));
    await waitFor(() => app.appState()?.platform.symlinks.supported === true);
    const escaped = app
      .planView()!
      .blocked.filter((b) => b.reason === "unsafeVaultPath");
    expect(escaped.length).toBe(1);

    const text = document.body.textContent ?? "";
    expect(text).toContain("extra_model_paths.yaml");
    expect(text).toContain("points back out of the vault");
    expect(text).toContain("Fix the category name in that file and scan again");
    // It says which install's file, because the person has two of them.
    expect(text).toContain("that ComfyUI-Sandbox uses");
    expect(text).not.toContain("unsafeVaultPath");
  });

  it("never plans a group for a file whose folder name would escape", async () => {
    const { app } = await mount(clearMachine);
    await waitFor(() => app.gate().can);
    const escaped = app.planView()!.blocked.find(
      (b) => b.reason === "unsafeVaultPath",
    )!;
    for (const group of app.plan()!.groups) {
      expect(group.source.absPath).not.toBe(escaped.absPath);
      for (const link of group.links) {
        expect(link.absPath).not.toBe(escaped.absPath);
      }
    }
  });

  it("says why a path frees nothing when it is a second name for one file", async () => {
    const { app } = await mount(clearMachine);
    await waitFor(() => app.gate().can);
    const group = app
      .plan()!
      .groups.find((g) => g.links.some((l) => l.sharesBytesWithAnother))!;
    expect(group, "the fixture must hold this case").toBeDefined();

    // The row itself says it, not some heading elsewhere on the screen, so
    // the total never quietly disagrees with the paths listed above it.
    const row = [...document.querySelectorAll(".grp")].find((el) =>
      el.textContent?.includes(fileNameOf(group.vaultRelPath)),
    )!;
    expect(row, "the group must be on screen").toBeDefined();
    const frees = [...row.querySelectorAll(".cp .to")].map((el) => el.textContent);
    expect(frees.filter((t) => t === "frees nothing")).toHaveLength(
      group.occurrences - group.distinctFiles,
    );
    expect(row.querySelector(".grp-why")!.textContent).toContain(
      "is a second name for a file already listed",
    );
    expect(row.querySelector(".grp-why")!.textContent).toContain("returns no space");
    // And no engine token reaches the glass.
    const screenText = document.body.textContent ?? "";
    expect(screenText).not.toContain("sharesBytesWithAnother");
    expect(screenText).not.toContain("distinctFiles");
  });

  it("does not repeat the links-are-off message once per file", async () => {
    const { app } = await mount();
    const text = document.body.textContent ?? "";
    const panel = document.querySelector(".blk")!.textContent ?? "";
    // It appears in the panel, and nowhere else.
    expect(panel).toContain("Windows needs Developer Mode");
    expect(
      text.split("Windows needs Developer Mode").length - 1,
      "the same sentence must not be repeated",
    ).toBe(1);
    for (const row of app.planView()!.blocked) {
      expect(row.reason).not.toBe("symlinkUnsupported");
    }
  });

  it("says why it kept the copy it kept", async () => {
    const { app } = await mount(clearMachine);
    await waitFor(() => app.gate().can);
    const text = document.body.textContent ?? "";
    expect(text).toContain("kept the copy in");
    expect(text).toContain("so moving it is a rename and takes no time");
  });
});

describe("an apply with nothing ticked", () => {
  it("is refused by the same rule that greys the button", async () => {
    const { app } = await mount(clearMachine);
    await waitFor(() => app.gate().can);

    for (const group of app.plan()!.groups) app.actions.toggleGroup(group.groupId);
    await waitFor(() => app.selection().moves === 0);

    // One rule, read in both places: the button is dead and the run refuses.
    expect(app.gate()).toMatchObject({ can: false, reason: "nothing_ticked" });
    const button = applyButton();
    expect(button).toBeDisabled();
    expect(button.textContent).toContain("Nothing is ticked");
  });

  it("cannot start even when the machine is otherwise clear", async () => {
    const { app } = await mount(clearMachine);
    await waitFor(() => app.gate().can);
    for (const group of app.plan()!.groups) app.actions.toggleGroup(group.groupId);
    await waitFor(() => app.selection().moves === 0);

    let started = 0;
    const original = app.engine.startApply.bind(app.engine);
    app.engine.startApply = async (args) => {
      started += 1;
      return original(args);
    };
    await userEvent.click(applyButton());
    expect(started).toBe(0);
    expect(app.applyProgress()).toBeNull();
  });
});
