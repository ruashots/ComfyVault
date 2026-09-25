import { describe, expect, it } from "vitest";

import {
  buildInstallViews,
  buildNameGroupView,
  buildPlanView,
  chosenBecauseText,
  fileNameOf,
  isAtLeast,
  placesOf,
  thumbnailStateOf,
} from "~/domain/view";
import { FixtureEngine } from "~/ipc/fixture/engine";
import type {
  ConsolidationPlan,
  Install,
  NameGroup,
  PlanGroup,
  ScanTotals,
} from "~/ipc/contract";

// ── the plan the engine hands over, arranged for the screen ─────────────────

async function readyPlan(): Promise<{
  engine: FixtureEngine;
  plan: ConsolidationPlan;
  totals: ScanTotals;
}> {
  const engine = new FixtureEngine();
  engine.devSetSymlinksSupported(true);
  engine.devSetComfyRunning(false);
  const scan = (await engine.getLastScan())!;
  const plan = await engine.buildPlan(scan.scanId);
  return { engine, plan, totals: scan.totals };
}

describe("the plan the screen shows", () => {
  it("keeps one group per unique content, and no group twice", async () => {
    const { plan } = await readyPlan();
    const ids = plan.groups.map((g) => g.sha256);
    expect(new Set(ids).size).toBe(ids.length);
    expect(plan.groups.length).toBe(plan.totals.groups);
  });

  it("agrees with its own totals", async () => {
    const { plan } = await readyPlan();
    expect(plan.groups.reduce((s, g) => s + g.bytesFreed, 0)).toBe(
      plan.totals.bytesFreed,
    );
    expect(plan.groups.filter((g) => g.bytesFreed > 0).length).toBe(
      plan.totals.groupsFreeingSpace,
    );
    expect(plan.groups.filter((g) => g.singleCopy).length).toBe(
      plan.totals.singleCopyGroups,
    );
    expect(plan.groups.reduce((s, g) => s + g.occurrences, 0)).toBe(
      plan.totals.linksCreated,
    );
  });

  it("frees one file's size for every real file after the first", async () => {
    const { plan } = await readyPlan();
    for (const group of plan.groups) {
      // Real files, not paths. Two names for one file free nothing.
      expect(group.bytesFreed, group.vaultRelPath).toBe(
        (group.distinctFiles - 1) * group.sizeBytes,
      );
      expect(group.singleCopy).toBe(group.occurrences === 1);
      expect(group.distinctFiles).toBeLessThanOrEqual(group.occurrences);
    }
  });

  it("counts a second name for one file as one file, not two", async () => {
    const { plan } = await readyPlan();
    const shared = plan.groups.filter((g) =>
      g.links.some((l) => l.sharesBytesWithAnother),
    );
    expect(shared.length, "the fixture must hold this case").toBeGreaterThan(0);
    for (const group of shared) {
      const extraNames = group.links.filter((l) => l.sharesBytesWithAnother).length;
      expect(group.distinctFiles).toBe(group.occurrences - extraNames);
      // Every path still gets a link. Only the space they return differs.
      expect(group.links).toHaveLength(group.occurrences);
    }
  });

  it("puts the biggest win first and lists singles separately", async () => {
    const { plan, totals } = await readyPlan();
    const view = buildPlanView(plan, totals);
    for (let i = 1; i < view.duplicates.length; i++) {
      expect(view.duplicates[i - 1]!.bytesFreed).toBeGreaterThanOrEqual(
        view.duplicates[i]!.bytesFreed,
      );
    }
    for (const group of view.duplicates) expect(group.singleCopy).toBe(false);
    for (const group of view.singles) expect(group.bytesFreed).toBe(0);
  });

  it("names the copy it keeps, and it is one of the copies", async () => {
    const { plan } = await readyPlan();
    for (const group of plan.groups) {
      expect(
        group.links.some((l) => l.absPath === group.source.absPath),
        group.vaultRelPath,
      ).toBe(true);
    }
  });

  it("pairs a renamed file with the one that took the plain name", async () => {
    const { plan, totals } = await readyPlan();
    const view = buildPlanView(plan, totals);
    expect(view.clashes.length).toBeGreaterThan(0);
    for (const clash of view.clashes) {
      expect(clash.groups.length).toBeGreaterThan(1);
      expect(clash.groups[0]!.vaultNameAdjusted).toBe(false);
      for (const later of clash.groups.slice(1)) {
        expect(later.vaultNameAdjusted).toBe(true);
        expect(later.clashesWith).toBe(clash.groups[0]!.sha256);
        expect(fileNameOf(later.vaultRelPath)).not.toBe(clash.filename);
      }
    }
  });

  it("counts blocked bytes from the rows it actually lists", async () => {
    const engine = new FixtureEngine();
    engine.devSetSymlinksSupported(true);
    const scan = (await engine.getLastScan())!;
    const view = buildPlanView(await engine.buildPlan(scan.scanId), scan.totals);
    expect(view.blockedBytes).toBe(
      view.blocked.reduce((sum, row) => sum + row.sizeBytes, 0),
    );
  });

  it("keeps counted-never-moved out of the list of problems", async () => {
    const { plan, totals } = await readyPlan();
    const view = buildPlanView(plan, totals);
    for (const row of view.blocked) {
      expect(row.reason).not.toBe("inCustomNodes");
      expect(row.reason).not.toBe("inHuggingFaceCache");
      expect(row.reason).not.toBe("symlinkUnsupported");
    }
    expect(view.countedNeverMoved.map((c) => c.kind)).toEqual([
      "custom_nodes",
      "huggingface_cache",
    ]);
  });

  it("groups what cannot move by reason", async () => {
    const engine = new FixtureEngine();
    engine.devSetSymlinksSupported(true);
    const scan = (await engine.getLastScan())!;
    const view = buildPlanView(await engine.buildPlan(scan.scanId), scan.totals);
    const reasons = view.blocked.map((r) => r.reason);
    expect(reasons).toContain("fileLocked");
    expect(reasons).toContain("permissionDenied");
    // Each reason appears in one run, not scattered.
    const firstSeen = new Map<(typeof reasons)[number], number>();
    reasons.forEach((reason, i) => {
      if (!firstSeen.has(reason)) firstSeen.set(reason, i);
    });
    for (const [reason, first] of firstSeen) {
      const last = reasons.lastIndexOf(reason);
      const count = reasons.filter((r) => r === reason).length;
      expect(last - first + 1, reason).toBe(count);
    }
  });
});

describe("a plan built while links are unavailable", () => {
  it("still says what would happen, and still blocks Apply", async () => {
    const engine = new FixtureEngine();
    const scan = (await engine.getLastScan())!;
    const plan = await engine.buildPlan(scan.scanId);
    expect(plan.symlinksSupported).toBe(false);
    expect(plan.groups.length).toBeGreaterThan(0);
    expect(plan.totals.bytesFreed).toBeGreaterThan(0);
  });

  it("says links are off once, not once per file", async () => {
    const engine = new FixtureEngine();
    const scan = (await engine.getLastScan())!;
    const plan = await engine.buildPlan(scan.scanId);
    const rows = plan.blocked.filter((b) => b.reason === "symlinkUnsupported");
    expect(rows).toHaveLength(1);
    expect(rows[0]!.absPath).toBe(plan.vaultRoot);
    expect(rows[0]!.sha256).toBeNull();
  });
});

// ── why a copy was chosen ───────────────────────────────────────────────────

function group(over: Partial<PlanGroup> = {}): PlanGroup {
  return {
    groupId: "g1",
    sha256: "A".repeat(64),
    sizeBytes: 100,
    category: "loras",
    vaultRelPath: "loras\\x.safetensors",
    vaultNameAdjusted: false,
    clashesWith: null,
    vaultAliases: [],
    distinctFiles: 2,
    source: {
      installId: "a",
      installLabel: "Studio",
      absPath: "C:\\a\\models\\loras\\x.safetensors",
      relPath: "models\\loras\\x.safetensors",
      sameVolumeAsVault: true,
      chosenBecause: "sameVolume",
      sizeBytes: 100,
      mtimeNanos: "1757491200000000000",
      ...over.source,
    },
    links: [],
    occurrences: 1,
    bytesFreed: 0,
    singleCopy: true,
    crossVolume: false,
    ...over,
  };
}

describe("why the engine kept that copy", () => {
  it("says so in words, never in the engine's own token", () => {
    const sameVolume = chosenBecauseText(group(), "C:");
    expect(sameVolume).toContain("Studio");
    expect(sameVolume).toContain("drive C:");
    expect(sameVolume).not.toContain("sameVolume");

    const only = chosenBecauseText(
      group({ source: { ...group().source, chosenBecause: "onlyCopy" } }),
      "C:",
    );
    expect(only).toContain("the only one there is");
    expect(only).not.toContain("onlyCopy");

    const byPath = chosenBecauseText(
      group({ source: { ...group().source, chosenBecause: "firstByPath" } }),
      "C:",
    );
    expect(byPath).toContain("first by path");
    expect(byPath).not.toContain("firstByPath");
  });
});

// ── the library ─────────────────────────────────────────────────────────────

describe("every place one content is reachable from", () => {
  it("lists each place once, and marks the copy whose bytes move", async () => {
    const { plan } = await readyPlan();
    const group = plan.groups.find((g) => g.occurrences > 1)!;
    const labels = new Map([
      ["studio", "Studio"],
      ["sandbox", "Sandbox"],
    ]);
    const places = placesOf(group.sha256, plan, [], labels);
    expect(places).toHaveLength(group.occurrences);
    expect(places.filter((p) => p.kind === "source")).toHaveLength(1);
    expect(new Set(places.map((p) => p.absPath)).size).toBe(places.length);
    expect(places.find((p) => p.kind === "source")!.absPath).toBe(
      group.source.absPath,
    );
  });

  it("adds the places that cannot move, with the reason", async () => {
    const engine = new FixtureEngine();
    engine.devSetSymlinksSupported(true);
    const scan = (await engine.getLastScan())!;
    const plan = await engine.buildPlan(scan.scanId);
    const stuck = plan.blocked.find((b) => b.reason === "fileLocked")!;
    const places = placesOf(stuck.sha256!, plan, [], new Map());
    const held = places.find((p) => p.absPath === stuck.absPath)!;
    expect(held.kind).toBe("stays");
    expect(held.blocked!.reason).toBe("fileLocked");
  });

  it("never counts a link twice when the vault and the plan both name it", async () => {
    const { plan } = await readyPlan();
    const group = plan.groups[0]!;
    const asLink = {
      id: "l1",
      installId: group.links[0]!.installId,
      absPath: group.links[0]!.absPath,
      relPath: group.links[0]!.relPath,
      linkName: group.links[0]!.linkName,
      sha256: group.sha256,
      vaultRelPath: group.vaultRelPath,
      createdAt: "2026-09-20T00:00:00.000Z",
      createdBy: "apply" as const,
      applyId: null,
    };
    const places = placesOf(group.sha256, plan, [asLink], new Map());
    expect(new Set(places.map((p) => p.absPath)).size).toBe(places.length);
    expect(places.filter((p) => p.absPath === asLink.absPath)).toHaveLength(1);
    expect(places.find((p) => p.absPath === asLink.absPath)!.kind).toBe("isLink");
  });
});

// ── installs ────────────────────────────────────────────────────────────────

describe("what each install gives up", () => {
  it("counts each path once, never the kept copy twice", async () => {
    const { engine, plan } = await readyPlan();
    const installs = await engine.listInstalls();
    const views = buildInstallViews(installs, plan, new Set());
    const totalMoving = views.reduce((s, v) => s + v.moving, 0);
    expect(totalMoving).toBe(plan.totals.linksCreated);
    for (const view of views) {
      expect(view.moving, view.install.label).toBeLessThanOrEqual(view.files);
    }
  });

  it("marks a running install as running", async () => {
    const { engine, plan } = await readyPlan();
    const installs = await engine.listInstalls();
    const views = buildInstallViews(installs, plan, new Set(["studio"]));
    expect(views.find((v) => v.install.id === "studio")!.running).toBe(true);
    expect(views.find((v) => v.install.id === "sandbox")!.running).toBe(false);
  });
});

describe("which ComfyUI versions lose model thumbnails", () => {
  it("is 0.28.0 and later, and nothing before it", () => {
    expect(isAtLeast("0.28.0", [0, 28, 0])).toBe(true);
    expect(isAtLeast("0.29.1", [0, 28, 0])).toBe(true);
    expect(isAtLeast("1.0.0", [0, 28, 0])).toBe(true);
    expect(isAtLeast("v0.28.0", [0, 28, 0])).toBe(true);
    expect(isAtLeast("0.27.9", [0, 28, 0])).toBe(false);
    expect(isAtLeast("0.9.7", [0, 28, 0])).toBe(false);
  });

  it("never calls an install fine when it does not say which version it runs", () => {
    // ComfyUI only began recording its version in 0.3.11, so an older install
    // cannot answer. Not knowing is not the same as knowing it is fine.
    expect(thumbnailStateOf(null)).toBe("unknown");
    expect(thumbnailStateOf("")).toBe("unknown");
    expect(thumbnailStateOf("not a version")).toBe("unknown");
    expect(thumbnailStateOf("0.28.0")).toBe("affected");
    expect(thumbnailStateOf("v0.37.0")).toBe("affected");
    expect(thumbnailStateOf("0.27.4")).toBe("unaffected");
    expect(thumbnailStateOf("0.3.10")).toBe("unaffected");
  });

  it("flags the install that runs one", async () => {
    const engine = new FixtureEngine();
    const installs: Install[] = await engine.listInstalls();
    const views = buildInstallViews(installs, null, new Set());
    expect(views.find((v) => v.install.id === "studio")!.thumbnails).toBe("affected");
    expect(views.find((v) => v.install.id === "sandbox")!.thumbnails).toBe("unaffected");
  });

  it("carries the unknown answer through to the view", async () => {
    const engine = new FixtureEngine();
    const installs = (await engine.listInstalls()).map((i) => ({
      ...i,
      version: null,
      versionSource: null,
    }));
    const views = buildInstallViews(installs, null, new Set());
    for (const view of views) expect(view.thumbnails).toBe("unknown");
  });
});

// ── cleanup ─────────────────────────────────────────────────────────────────

function nameGroup(names: NameGroup["names"]): NameGroup {
  return {
    sha256: "C".repeat(64),
    sizeBytes: 100,
    category: "vae",
    canonicalName: names.find((n) => n.isCanonical)?.name ?? names[0]!.name,
    names,
  };
}

describe("which name the vault should keep", () => {
  it("prefers the name the most links already resolve through", () => {
    const view = buildNameGroupView(
      nameGroup([
        { name: "a.safetensors", isCanonical: true, vaultRelPath: "vae/a", usedByLinks: 1, seenInInstalls: ["studio"] },
        { name: "bbbbbb.safetensors", isCanonical: false, vaultRelPath: "vae/b", usedByLinks: 4, seenInInstalls: ["sandbox"] },
      ]),
    );
    expect(view.suggestion.name).toBe("bbbbbb.safetensors");
    expect(view.suggestion.reason).toContain("4 links point at this name already");
  });

  it("says one link, not one links", () => {
    const view = buildNameGroupView(
      nameGroup([
        { name: "a.safetensors", isCanonical: true, vaultRelPath: "vae/a", usedByLinks: 1, seenInInstalls: ["studio"] },
        { name: "b.safetensors", isCanonical: false, vaultRelPath: "vae/b", usedByLinks: 0, seenInInstalls: [] },
      ]),
    );
    expect(view.suggestion.reason).toBe(
      "1 link points at this name and none at the other",
    );
  });

  it("says so plainly when the links are level and only the length separates them", () => {
    const view = buildNameGroupView(
      nameGroup([
        { name: "a.safetensors", isCanonical: true, vaultRelPath: "vae/a", usedByLinks: 1, seenInInstalls: ["studio"] },
        { name: "a-longer.safetensors", isCanonical: false, vaultRelPath: "vae/b", usedByLinks: 1, seenInInstalls: ["sandbox"] },
      ]),
    );
    expect(view.suggestion.name).toBe("a-longer.safetensors");
    expect(view.suggestion.reason).toBe(
      "the same number of links point at either, so this is the longer name",
    );
  });

  it("falls back to the installs that use it", () => {
    const view = buildNameGroupView(
      nameGroup([
        { name: "a.safetensors", isCanonical: true, vaultRelPath: "vae/a", usedByLinks: 0, seenInInstalls: [] },
        { name: "b.safetensors", isCanonical: false, vaultRelPath: "vae/b", usedByLinks: 0, seenInInstalls: ["Sandbox"] },
      ]),
    );
    expect(view.suggestion.name).toBe("b.safetensors");
    expect(view.suggestion.reason).toContain("Sandbox");
  });

  it("falls back to the longer name when nothing else separates them", () => {
    const view = buildNameGroupView(
      nameGroup([
        { name: "short.safetensors", isCanonical: true, vaultRelPath: "vae/s", usedByLinks: 0, seenInInstalls: [] },
        { name: "a-much-longer-name.safetensors", isCanonical: false, vaultRelPath: "vae/l", usedByLinks: 0, seenInInstalls: [] },
      ]),
    );
    expect(view.suggestion.name).toBe("a-much-longer-name.safetensors");
    expect(view.suggestion.reason).toContain("longer name");
  });

  it("only offers to remove a name nothing uses", () => {
    const view = buildNameGroupView(
      nameGroup([
        { name: "keep.safetensors", isCanonical: true, vaultRelPath: "vae/k", usedByLinks: 2, seenInInstalls: ["studio"] },
        { name: "used.safetensors", isCanonical: false, vaultRelPath: "vae/u", usedByLinks: 1, seenInInstalls: ["sandbox"] },
        { name: "spare.safetensors", isCanonical: false, vaultRelPath: "vae/s", usedByLinks: 0, seenInInstalls: [] },
      ]),
    );
    expect(view.choices.map((c) => c.removable)).toEqual([false, false, true]);
  });
});
