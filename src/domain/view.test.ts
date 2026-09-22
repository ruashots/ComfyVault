import { describe, expect, it } from "vitest";

import {
  buildInstallViews,
  buildLibrary,
  buildNameGroupView,
  buildPlanView,
  chosenBecauseText,
  fileNameOf,
  isAtLeast,
} from "~/domain/view";
import { FixtureEngine } from "~/ipc/fixture/engine";
import type {
  BlockedRow,
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

  it("frees one file's size for every copy after the first", async () => {
    const { plan } = await readyPlan();
    for (const group of plan.groups) {
      expect(group.bytesFreed, group.vaultRelPath).toBe(
        (group.occurrences - 1) * group.sizeBytes,
      );
      expect(group.singleCopy).toBe(group.occurrences === 1);
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
    expect(plan.groups.length).toBeGreaterThan(0);
    expect(plan.totals.bytesFreed).toBeGreaterThan(0);
    expect(plan.blocked.some((b) => b.reason === "symlinkUnsupported")).toBe(true);
  });
});

// ── why a copy was chosen ───────────────────────────────────────────────────

function group(over: Partial<PlanGroup> = {}): PlanGroup {
  return {
    groupId: "g1",
    sha256: "A".repeat(64),
    sizeBytes: 100,
    category: "loras",
    vaultRelPath: "loras/x.safetensors",
    vaultNameAdjusted: false,
    clashesWith: null,
    source: {
      installId: "a",
      installLabel: "Production",
      absPath: "C:\\a\\models\\loras\\x.safetensors",
      relPath: "models\\loras\\x.safetensors",
      sameVolumeAsVault: true,
      chosenBecause: "sameVolume",
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
    expect(sameVolume).toContain("Production");
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

describe("one row per unique content", () => {
  it("lists a plan group and a vault file as one row, not two", async () => {
    const { engine, plan } = await readyPlan();
    const vaultFiles = (await engine.listVaultFiles({ offset: 0, limit: 1000 }))
      .files;
    const rows = buildLibrary(plan, vaultFiles);
    const hashes = rows.map((r) => r.sha256);
    expect(new Set(hashes).size).toBe(hashes.length);
    expect(rows.length).toBe(plan.groups.length + vaultFiles.length);
  });

  it("lists the kept copy once, not once as the source and once as a link", async () => {
    const { engine, plan } = await readyPlan();
    const vaultFiles = (await engine.listVaultFiles({ offset: 0, limit: 1000 }))
      .files;
    const rows = buildLibrary(plan, vaultFiles);
    for (const row of rows) {
      const paths = row.places.map((p) => p.absPath);
      expect(new Set(paths).size, row.name).toBe(paths.length);
      expect(row.places.filter((p) => p.kind === "source").length).toBeLessThan(2);
    }
    const group = plan.groups.find((g) => g.occurrences > 1)!;
    const row = rows.find((r) => r.sha256 === group.sha256)!;
    expect(row.places).toHaveLength(group.occurrences);
  });

  it("shows a vault file nothing points at as an orphan", async () => {
    const { engine, plan } = await readyPlan();
    const vaultFiles = (await engine.listVaultFiles({ offset: 0, limit: 1000 }))
      .files;
    const rows = buildLibrary(plan, vaultFiles);
    const orphans = rows.filter((r) => r.isOrphan);
    expect(orphans.length).toBe(3);
    for (const orphan of orphans) {
      expect(orphan.places).toHaveLength(0);
      expect(orphan.inVaultSince).not.toBeNull();
    }
  });

  it("gives every place a group covers, including the one that stays put", async () => {
    const engine = new FixtureEngine();
    engine.devSetSymlinksSupported(true);
    const scan = (await engine.getLastScan())!;
    const plan = await engine.buildPlan(scan.scanId);
    const rows = buildLibrary(plan, []);
    const withBlocked = rows.filter((r) =>
      r.places.some((p) => p.blocked !== null),
    );
    expect(withBlocked.length).toBeGreaterThan(0);
    for (const row of withBlocked) {
      expect(row.places.some((p) => p.kind === "stays")).toBe(true);
    }
  });

  it("finds a file that only appears as something that cannot move", () => {
    const blocked: BlockedRow = {
      absPath: "C:\\a\\models\\loras\\stuck.safetensors",
      installId: "a",
      installLabel: "Production",
      sizeBytes: 500,
      sha256: "B".repeat(64),
      reason: "permissionDenied",
      detail: "denied",
    };
    const rows = buildLibrary(
      {
        planId: "p",
        scanId: "s",
        createdAt: "2026-09-22T00:00:00.000Z",
        vaultRoot: "C:\\ComfyVault",
        groups: [],
        blocked: [blocked],
        totals: {
          groups: 0,
          groupsFreeingSpace: 0,
          singleCopyGroups: 0,
          nameClashes: 0,
          crossVolumeGroups: 0,
          bytesFreed: 0,
          bytesMoved: 0,
          filesMoved: 0,
          linksCreated: 0,
          blockedRows: 1,
          blockedBytes: 500,
          vaultFreeBytesAfter: 0,
        },
      },
      [],
    );
    expect(rows).toHaveLength(1);
    expect(rows[0]!.name).toBe("stuck.safetensors");
    expect(rows[0]!.places[0]!.blocked?.reason).toBe("permissionDenied");
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
    const views = buildInstallViews(installs, plan, new Set(["prod"]));
    expect(views.find((v) => v.install.id === "prod")!.running).toBe(true);
    expect(views.find((v) => v.install.id === "norm")!.running).toBe(false);
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
    expect(isAtLeast(null, [0, 28, 0])).toBe(false);
    expect(isAtLeast("not a version", [0, 28, 0])).toBe(false);
  });

  it("flags the install that runs one", async () => {
    const engine = new FixtureEngine();
    const installs: Install[] = await engine.listInstalls();
    const views = buildInstallViews(installs, null, new Set());
    expect(views.find((v) => v.install.id === "prod")!.thumbnailsAffected).toBe(true);
    expect(views.find((v) => v.install.id === "norm")!.thumbnailsAffected).toBe(false);
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
        { name: "a.safetensors", isCanonical: true, vaultRelPath: "vae/a", usedByLinks: 1, seenInInstalls: ["prod"] },
        { name: "bbbbbb.safetensors", isCanonical: false, vaultRelPath: "vae/b", usedByLinks: 4, seenInInstalls: ["norm"] },
      ]),
    );
    expect(view.suggestion.name).toBe("bbbbbb.safetensors");
    expect(view.suggestion.reason).toContain("4 links point at this name already");
  });

  it("says one link, not one links", () => {
    const view = buildNameGroupView(
      nameGroup([
        { name: "a.safetensors", isCanonical: true, vaultRelPath: "vae/a", usedByLinks: 1, seenInInstalls: ["prod"] },
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
        { name: "a.safetensors", isCanonical: true, vaultRelPath: "vae/a", usedByLinks: 1, seenInInstalls: ["prod"] },
        { name: "a-longer.safetensors", isCanonical: false, vaultRelPath: "vae/b", usedByLinks: 1, seenInInstalls: ["norm"] },
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
        { name: "b.safetensors", isCanonical: false, vaultRelPath: "vae/b", usedByLinks: 0, seenInInstalls: ["Normal"] },
      ]),
    );
    expect(view.suggestion.name).toBe("b.safetensors");
    expect(view.suggestion.reason).toContain("Normal");
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
        { name: "keep.safetensors", isCanonical: true, vaultRelPath: "vae/k", usedByLinks: 2, seenInInstalls: ["prod"] },
        { name: "used.safetensors", isCanonical: false, vaultRelPath: "vae/u", usedByLinks: 1, seenInInstalls: ["norm"] },
        { name: "spare.safetensors", isCanonical: false, vaultRelPath: "vae/s", usedByLinks: 0, seenInInstalls: [] },
      ]),
    );
    expect(view.choices.map((c) => c.removable)).toEqual([false, false, true]);
  });
});
