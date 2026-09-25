import { screen } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";

import { ConfirmModalView } from "~/modals/confirm";
import { CleanupScreen } from "~/screens/Cleanup";
import { HomeScreen } from "~/screens/Home";
import { LibraryScreen } from "~/screens/Library";
import { App } from "~/App";
import { FixtureEngine } from "~/ipc/fixture/engine";
import { renderWithApp, waitFor, type Harness } from "~/test/render";

let harness: Harness | null = null;

afterEach(() => {
  harness?.unmount();
  harness = null;
});

/** Run once, then take a file out of the vault from underneath its links. */
async function withBrokenLinks(screenUnderTest: () => ReturnType<typeof HomeScreen>) {
  const engine = new FixtureEngine({ manual: true });
  engine.devSetSymlinksSupported(true);
  engine.devSetComfyRunning(false);
  const scan = (await engine.getLastScan())!;
  const plan = await engine.buildPlan(scan.scanId);
  await engine.startApply({
    planId: plan.planId,
    groupIds: plan.groups.slice(0, 3).map((g) => g.groupId),
  });
  // Run it to the end rather than waiting a guessed number of milliseconds.
  engine.devFinish();
  const broken = engine.devBreakLinks(1);
  expect(broken).toBeGreaterThan(0);

  harness = await renderWithApp(
    () => (
      <>
        {screenUnderTest()}
        <ConfirmModalView />
      </>
    ),
    { engine },
  );
  await waitFor(() => harness!.app.danglingLinks().length > 0);
  return harness;
}

describe("a link that points at a file that is not there", () => {
  it(
    "is the first thing Home says, and offers to remove it",
    async () => {
      const { app } = await withBrokenLinks(() => <HomeScreen />);
      const panel = document.querySelector(".blk")!;
      expect(panel.textContent).toContain("at a file that is not there");
      expect(panel.textContent).toContain(
        "ComfyUI will list each of these in its model dropdown and then fail to load it",
      );
      expect(panel.textContent).toContain(
        "write straight through the broken link",
      );
      // It comes before the figures.
      const scroll = document.querySelector(".scroll")!;
      expect(scroll.firstElementChild).toBe(panel);
      expect(screen.getAllByRole("button", { name: /Remove/ }).length).toBeGreaterThan(0);
      expect(app.danglingLinks().length).toBeGreaterThan(0);
    },
    15_000,
  );

  it(
    "goes away once the broken link is removed",
    async () => {
      const { app } = await withBrokenLinks(() => <CleanupScreen />);
      const before = app.danglingLinks().length;
      await userEvent.click(
        screen.getAllByRole("button", { name: "Remove this one" })[0]!,
      );
      await waitFor(() => app.danglingLinks().length === before - 1);
      expect(app.danglingLinks().length).toBe(before - 1);
    },
    15_000,
  );

  it(
    "removes every one of them when asked, and says nothing is lost",
    async () => {
      const { app } = await withBrokenLinks(() => <CleanupScreen />);
      await userEvent.click(
        screen.getByRole("button", { name: /Remove (it|them all)/ }),
      );
      await waitFor(() => document.querySelector(".modal") !== null);
      const modal = document.querySelector(".modal")!;
      expect(modal.textContent).toContain("no model file is deleted");
      expect(modal.textContent).toContain("points at nothing");
      await userEvent.click(screen.getByRole("button", { name: /^Remove \d+ links$/ }));
      await waitFor(() => app.danglingLinks().length === 0);
      expect(document.querySelector(".blk")).toBeNull();
    },
    15_000,
  );
});

describe("what the workflow check actually did", () => {
  it("is said next to the list of models nothing names", async () => {
    const engine = new FixtureEngine();
    harness = await renderWithApp(() => <LibraryScreen />, { engine });
    await waitFor(() => harness!.app.usage().size > 0);
    harness.app.setLib("unusedOnly", true);
    await waitFor(() => document.querySelector(".lib-method") !== null);
    const note = document.querySelector(".lib-method")!;
    expect(note.textContent).toContain(
      "The file name was searched for as plain text inside saved workflow files.",
    );
    expect(note.textContent).toContain("not a list of models that are safe to delete");
  });

  it("is said next to the answer for one model", async () => {
    const engine = new FixtureEngine();
    harness = await renderWithApp(() => <LibraryScreen />, { engine });
    await waitFor(() => harness!.app.usage().size > 0);
    const row = harness.app.library()[0]!;
    harness.app.setLib({ selected: row.sha256, drawerOpen: true });
    await waitFor(() => document.querySelector(".drawer") !== null);
    expect(document.querySelector(".drawer")!.textContent).toContain(
      "The file name was searched for as plain text inside saved workflow files.",
    );
  });
});

describe("when there was no saved workflow file to search", () => {
  it("shows no count of unused models on Home, because nothing was searched", async () => {
    const engine = new FixtureEngine();
    engine.devSetWorkflowsOnDisk(0);
    harness = await renderWithApp(() => <App />, { engine });
    await waitFor(() => harness!.app.usage().size > 0);
    await waitFor(() => harness!.app.nothingSearched());
    harness.app.actions.go("home");
    await waitFor(() => (document.body.textContent ?? "").includes("Instances"));

    const tile = [...document.querySelectorAll(".tile")].find((t) =>
      (t.textContent ?? "").includes("Not used"),
    )!;
    const text = (tile.textContent ?? "").replace(/\s+/g, " ");
    expect(text).toContain("not known");
    expect(text).toContain("no saved workflow files to search");
    expect(text).not.toMatch(/\b0\b/);
    expect(text).not.toContain("name not found in any workflow");
  });

  it("counts unused models on Home once workflows were searched", async () => {
    harness = await renderWithApp(() => <App />, { engine: new FixtureEngine() });
    await waitFor(() => harness!.app.usage().size > 0);
    harness.app.actions.go("home");
    await waitFor(() => (document.body.textContent ?? "").includes("Instances"));
    const tile = [...document.querySelectorAll(".tile")].find((t) =>
      (t.textContent ?? "").includes("Not used"),
    )!;
    expect(tile.textContent).toContain(String(harness.app.unusedCount()));
    expect(tile.textContent).toContain("name not found in any workflow");
  });

  it("says so rather than calling every model unused", async () => {
    const engine = new FixtureEngine();
    engine.devSetWorkflowsOnDisk(0);
    harness = await renderWithApp(() => <LibraryScreen />, { engine });
    await waitFor(() => harness!.app.usage().size > 0);
    await waitFor(() => harness!.app.nothingSearched());

    // The filter is not offered, because there is no answer to filter on.
    expect(screen.queryByRole("button", { name: /Not used/ })).toBeNull();
    expect(harness.app.unusedCount()).toBe(0);
    // And no row is marked either way.
    expect(document.querySelector(".lrow .dot.unused")).toBeNull();
    expect(document.querySelector(".lrow .dot.used")).toBeNull();
  });

  it("says so in the drawer, in the engine's own words", async () => {
    const engine = new FixtureEngine();
    engine.devSetWorkflowsOnDisk(0);
    harness = await renderWithApp(() => <LibraryScreen />, { engine });
    await waitFor(() => harness!.app.usage().size > 0);
    const row = harness.app.library()[0]!;
    harness.app.setLib({ selected: row.sha256, drawerOpen: true });
    await waitFor(() => document.querySelector(".drawer") !== null);
    const drawer = document.querySelector(".drawer")!;
    expect(drawer.textContent).toContain(
      "No saved workflow files were found, so nothing was searched.",
    );
    expect(drawer.textContent).toContain("Nothing was checked for this model.");
    expect(drawer.textContent).not.toContain("appears in no saved workflow file");
    // The header must not answer either.
    expect(document.querySelector(".det-meta")!.textContent).not.toContain("Not used");
    expect(document.querySelector(".det-meta")!.textContent).not.toContain("In use");
  });
});

describe("a ComfyUI that will not show a picture for a linked model", () => {
  it("says so on the screen where the person decides", async () => {
    const engine = new FixtureEngine();
    engine.devSetSymlinksSupported(true);
    harness = await renderWithApp(() => <HomeScreen />, { engine });
    await waitFor(() => harness!.app.plan() !== null);
    const text = document.body.textContent ?? "";
    expect(text).toContain("will not show a preview thumbnail for a model reached");
    expect(text).toContain("Loading a model and running a workflow are not affected");
  });

  it("never reads as not affected when the version is unknown", async () => {
    const engine = new FixtureEngine();
    engine.devSetSymlinksSupported(true);
    engine.devForgetVersions();
    harness = await renderWithApp(() => <HomeScreen />, { engine });
    await waitFor(() => harness!.app.plan() !== null);
    await waitFor(() =>
      harness!.app.installViews().every((v) => v.thumbnails === "unknown"),
    );
    const text = document.body.textContent ?? "";
    expect(text).toContain("do not record which ComfyUI version they run");
    expect(text).toContain("so this is unknown there");
    // The fact must be on screen, not silently absent.
    expect(text).not.toContain("0.28.0 or newer, which will not show");
  });
});

describe("a vault whose drive stops answering", () => {
  it("says so rather than showing a full drive", async () => {
    const engine = new FixtureEngine();
    engine.devSetDriveReadable(false);
    harness = await renderWithApp(() => <App />, { engine });
    await waitFor(() => harness!.app.ready());

    // The rest of the answer still arrives, so one unreadable figure does not
    // take the screen down.
    expect(harness.app.failure()).toBeNull();
    expect(harness.app.vault()).not.toBeNull();
    expect(harness.app.vault()!.freeBytes).toBeNull();
    expect(harness.app.vault()!.totalBytes).toBeNull();
    expect(harness.app.installs().length).toBeGreaterThan(0);

    // No meter, because zero of zero would draw a completely full drive.
    expect(document.querySelector(".rail .meter")).toBeNull();
    const text = document.body.textContent ?? "";
    expect(text).not.toContain("0 B free");
    expect(text).not.toContain("100% full");
    expect(text).not.toContain("NaN");
  });

  it("keeps the same silence in Settings", async () => {
    const engine = new FixtureEngine();
    engine.devSetDriveReadable(false);
    harness = await renderWithApp(() => <App />, { engine });
    await waitFor(() => harness!.app.ready());
    harness.app.actions.go("settings");
    await waitFor(() => document.body.textContent?.includes("Free space") === true);

    const text = document.body.textContent ?? "";
    expect(text).toContain("did not answer when asked how much room it has");
    expect(text).not.toContain("0 B free");
  });
});
