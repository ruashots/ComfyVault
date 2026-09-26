import { screen } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";

import { fmt } from "~/domain/format";
import { fileNameOf } from "~/domain/view";
import type { ConsolidationPlan } from "~/ipc/contract";
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
    expect(bar.querySelector(".say")!.textContent).toBe(
      `${selection.links} copies will be replaced by links to ${selection.moves} vault files`,
    );
    expect(bar.textContent).toContain(`${fmt(selection.bytes)}to be freed on drive C:`);
    // Nothing on this bar says that links go back anywhere: there are none yet.
    expect(bar.textContent).not.toContain("back");
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
    expect(row.querySelector(".grp-why")!.textContent).toBe(
      "One of these copies is a second name for another one above, so it frees no space.",
    );
    // Only the second name has its own words: every other copy row is the same.
    expect(row.querySelectorAll(".cp")).toHaveLength(group.occurrences);
    expect(row.querySelector(".grp-s")!.textContent).toBe(
      `${fmt(group.sizeBytes * (group.distinctFiles - 1))}to be freed`,
    );
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

  it("never says which copy is kept: every copy is the same file", async () => {
    const { app } = await mount(clearMachine);
    await waitFor(() => app.gate().can);
    const text = document.body.textContent ?? "";
    expect(text).not.toContain("kept the copy");
    expect(text).not.toMatch(/\bkeep\b/);
    expect(document.querySelectorAll(".grp:not(.dead) .cp .role")).toHaveLength(0);
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

describe("installs that the engine gave the same name", () => {
  const sameLabels = (engine: FixtureEngine) => {
    void engine.updateInstall("studio", "ComfyUI");
    void engine.updateInstall("sandbox", "ComfyUI");
  };

  it("are told apart by their folders in the summary", async () => {
    await mount(sameLabels);
    const keys = [...document.querySelectorAll(".kv .k")].map((k) => k.textContent);
    expect(keys).toContain("ComfyUI-Studio");
    expect(keys).toContain("ComfyUI-Sandbox");
    expect(keys).not.toContain("ComfyUI");
    const studio = [...document.querySelectorAll(".kv .k")].find(
      (k) => k.textContent === "ComfyUI-Studio",
    )!;
    expect(studio.getAttribute("title")).toBe("C:\\ComfyUI-Studio");
  });

  it("are told apart in the list of single copies", async () => {
    const { app } = await mount(sameLabels);
    app.actions.setShowSingles(true);
    await waitFor(() => document.querySelectorAll(".grp .who").length > 0);
    const who = new Set(
      [...document.querySelectorAll(".grp .who")].map((w) => w.textContent),
    );
    expect(who.has("ComfyUI")).toBe(false);
    expect([...who].every((w) => w === "ComfyUI-Studio" || w === "ComfyUI-Sandbox")).toBe(true);
  });
});

/** Change the plan the engine builds, for a case the sample disk does not hold. */
const reshape = (fn: (plan: ConsolidationPlan) => ConsolidationPlan) => (engine: FixtureEngine) => {
  clearMachine(engine);
  const build = engine.buildPlan.bind(engine);
  engine.buildPlan = async (scanId) => fn(await build(scanId));
};

/** Mount with every model listed, not only the ten biggest. */
async function mountAll(prepare: (engine: FixtureEngine) => void) {
  const h = await mount(prepare);
  h.app.actions.setShowAllDuplicates(true);
  await waitFor(
    () => document.querySelectorAll(".grp-after").length === h.app.planView()!.duplicates.length,
  );
  return h;
}

const groupRow = (name: string) =>
  [...document.querySelectorAll(".grp")].find(
    (el) => el.querySelector(".grp-n")?.textContent === name,
  )!;

describe("the plan says what will happen, as a plan", () => {
  it("states the summary in the future", async () => {
    const { app } = await mount(clearMachine);
    const totals = app.plan()!.totals;
    const text = document.body.textContent ?? "";
    expect(text).toContain("A dry run. Nothing moves until you apply it.");
    expect(text).toContain("What this run will do");
    expect(text).toContain("nothing has moved yet");
    expect(text).toContain(
      `${fmt(totals.bytesFreed)} once ${totals.linksCreated} copies are replaced by links`,
    );
    expect(text).toContain(`in ${totals.filesMoved} files, one copy of each model`);
    const studio = app.installViews().find((v) => v.install.id === "studio")!;
    expect(text).toContain(
      `${studio.moving} copies will be replaced by links, and ${fmt(studio.movingBytes)} will leave C:\\ComfyUI-Studio`,
    );
    for (const old of ["Space returned", "become links", "The vault holds", "back on drive", "go back", "come back"]) {
      expect(text, old).not.toContain(old);
    }
  });

  it("titles each section with a phrase that says something", async () => {
    const { app } = await mount(clearMachine);
    const titles = [...document.querySelectorAll(".sec .t")].map((t) => t.textContent);
    expect(titles).toEqual([
      "What this run will do",
      "Same file, in more than one place",
      "Different files with the same name",
      "One copy only, so nothing will be freed yet",
      "Files that cannot move",
    ]);
    const counts = [...document.querySelectorAll(".sec .n")].map((t) => t.textContent);
    const view = app.planView()!;
    expect(counts[1]).toBe(
      `${view.duplicates.length} models, ${fmt(app.plan()!.totals.bytesFreed)} to be freed`,
    );
    expect(counts[2]).toBe(`${view.clashes.length} names`);
    // No facts strung together with a dot, in any count a person reads.
    for (const count of counts) expect(count).not.toContain("\u00b7");
  });

  it("lists every copy of a model the same way, as its full path", async () => {
    const { app } = await mount(clearMachine);
    const group = app.planView()!.duplicates.find(
      (g) =>
        g.links.every((l) => !l.sharesBytesWithAnother) &&
        g.occurrences === 2 &&
        !g.crossVolume &&
        new Set(g.links.map((l) => l.linkName)).size === 1,
    )!;
    const row = groupRow(fileNameOf(group.vaultRelPath));
    expect(row.querySelector(".grp-sub")!.textContent).toBe(
      `2 copies of the same ${fmt(group.sizeBytes)} file, SHA-256 ${group.sha256.slice(0, 8)}\u2026${group.sha256.slice(-8)}`,
    );
    const paths = [...row.querySelectorAll(".cp .pp")];
    expect(paths.map((p) => p.getAttribute("title"))).toEqual(group.links.map((l) => l.absPath));
    expect(paths.map((p) => p.textContent)).toEqual(group.links.map((l) => l.absPath));
    expect(row.querySelectorAll(".cp .to")).toHaveLength(0);
    expect(row.querySelector(".grp-after")!.textContent).toBe(
      `Both copies will be replaced by links to one file in the vault: vault\\${group.vaultRelPath}`,
    );
    // A same-drive model with one name needs no explaining.
    expect(row.querySelector(".grp-why")).toBeNull();
  });

  it("marks the names that differ when one model has two names", async () => {
    const { app } = await mountAll(clearMachine);
    const group = app.planView()!.duplicates.find(
      (g) => new Set(g.links.map((l) => l.linkName)).size === 2 && g.occurrences === 2,
    )!;
    expect(group, "the fixture must hold this case").toBeDefined();
    const row = groupRow(fileNameOf(group.vaultRelPath));
    const alt = [...row.querySelectorAll(".cp .pf.alt")].map((el) => el.textContent);
    expect(alt).toEqual(group.links.filter((l) => l.nameDiffersFromVault).map((l) => l.linkName));
    expect(alt.length).toBeGreaterThan(0);
    const why = row.querySelector(".grp-why")!;
    expect(why.querySelector(".alt")!.textContent).toBe("Two names for one model.");
    expect(why.textContent).toBe(
      "Two names for one model. Each install keeps the name it uses now, so its workflows still open. After the run, pick one name in Cleanup.",
    );
  });

  it("does not mark a name when every copy has the same one", async () => {
    await mount(
      reshape((plan) => ({
        ...plan,
        groups: plan.groups.map((g) =>
          g.links.length > 1
            ? { ...g, links: g.links.map((l) => ({ ...l, nameDiffersFromVault: true })) }
            : g,
        ),
      })),
    );
    // One name everywhere: nothing is amber, even when the engine flags it.
    const sameName = [...document.querySelectorAll(".grp")].filter(
      (g) => g.querySelector(".grp-why .alt") === null && g.querySelector(".grp-after"),
    );
    expect(sameName.length).toBeGreaterThan(0);
    for (const g of sameName) expect(g.querySelectorAll(".pf.alt")).toHaveLength(0);
  });

  it("points at the file the vault already holds after an earlier run", async () => {
    let name = "";
    await mountAll(
      reshape((plan) => {
        const g = plan.groups.find((x) => x.bytesFreed > 0)!;
        name = fileNameOf(g.vaultRelPath);
        return {
          ...plan,
          groups: plan.groups.map((x) => (x === g ? { ...x, alreadyInVault: true } : x)),
        };
      }),
    );
    expect(groupRow(name).querySelector(".grp-after")!.textContent).toContain(
      "Both copies will be replaced by links to the file the vault already holds:",
    );
  });

  it("says a copy crosses drives when no copy is on the vault drive", async () => {
    let name = "";
    await mountAll(
      reshape((plan) => {
        const g = plan.groups.find((x) => x.bytesFreed > 0 && !x.crossVolume)!;
        name = fileNameOf(g.vaultRelPath);
        return {
          ...plan,
          groups: plan.groups.map((x) =>
            x === g ? { ...x, source: { ...x.source, chosenBecause: "firstByPath" as const } } : x,
          ),
        };
      }),
    );
    expect(groupRow(name).querySelector(".grp-why")!.textContent).toBe(
      "No copy is on the vault drive, so one copy will be copied across and checked before anything is deleted.",
    );
  });

  it("lists a model whose vault name carries a code under the name the installs use", async () => {
    await mountAll(
      reshape((plan) => ({
        ...plan,
        groups: plan.groups.map((g) =>
          g.vaultNameAdjusted
            ? {
                ...g,
                links: [g.links[0]!, { ...g.links[0]!, absPath: `${g.links[0]!.absPath}.2`, isSource: false }],
                occurrences: 2,
                distinctFiles: 2,
                bytesFreed: g.sizeBytes,
                singleCopy: false,
              }
            : g,
        ),
      })),
    );
    const tagged = groupRow("model.safetensors");
    expect(tagged, "the tagged model is listed by the installs' name").toBeDefined();
    expect(tagged.querySelector(".grp-why")!.textContent).toContain(
      "The installs keep model.safetensors. See Different files with the same name, below.",
    );
    expect(tagged.querySelector(".grp-after")!.textContent).toMatch(
      /vault\\clip_vision\\model__[0-9A-F]{8}\.safetensors$/,
    );
  });

  it("names each model a shared name belongs to, and highlights the added code", async () => {
    const { app } = await mount(clearMachine);
    const clash = app.planView()!.clashes.find((c) => c.filename === "model.safetensors")!;
    const block = [...document.querySelectorAll(".cgrp")].find(
      (el) => el.querySelector(".grp-h.static .grp-n")?.textContent === "model.safetensors",
    )!;
    expect(block.querySelector(".clash-n")!.textContent).toBe("two different models");
    const lines = [...block.querySelectorAll(".cm-to")].map((el) => el.textContent);
    expect(lines[0]).toBe("into the vault as model.safetensors");
    expect(lines[1]).toBe(`into the vault as ${fileNameOf(clash.groups[1]!.vaultRelPath)}`);
    expect(block.querySelector(".cm-to .tag")!.textContent).toMatch(/^__[0-9A-F]{8}$/);
    expect(block.querySelectorAll(".cm .cp")).toHaveLength(
      clash.groups.reduce((s, g) => s + g.links.length, 0),
    );
  });

  it("unticks a model in every section it appears in at once", async () => {
    const { app } = await mount(clearMachine);
    const clash = app.planView()!.clashes[0]!;
    const second = clash.groups[1]!;
    const line = screen.getByRole("button", {
      name: `Include the ${fmt(second.sizeBytes)} ${clash.filename}`,
    });
    await userEvent.click(line);
    await waitFor(() => app.unticked().has(second.groupId));
    expect(line.closest(".cm")!.classList.contains("off")).toBe(true);
    app.actions.setShowSingles(true);
    await waitFor(() => document.querySelector(".grp.off") !== null);
  });

  it("lists single copies under the name the installs use", async () => {
    const { app } = await mount(clearMachine);
    app.actions.setShowSingles(true);
    await waitFor(() => screen.queryAllByRole("button", { name: /^Include / }).length > 0);
    const names = [...document.querySelectorAll(".grp .grp-n")].map((n) => n.textContent);
    expect(names.some((n) => /__[0-9A-F]{8}/.test(n ?? ""))).toBe(false);
  });

  it("says each section is empty in a sentence", async () => {
    await mount(reshape((plan) => ({ ...plan, groups: [], blocked: [] })));
    const text = document.body.textContent ?? "";
    expect(text).toContain("No model has more than one copy, so this run will free no space.");
    expect(text).toContain(
      "No two different files share a name. Every model will go into the vault under the name it has now.",
    );
    expect(text).toContain("No model has only one copy.");
    const counts = [...document.querySelectorAll(".sec .n")].map((t) => t.textContent);
    expect(counts[4]).toBe("none");
  });
});
