import { describe, expect, it } from "vitest";

import { derivePlan, reclaimIfComfyClosed } from "~/domain/plan";
import { fixtureMachine, fixtureScan } from "~/ipc/fixture/dataset";
import { BLESSED, MB } from "~/test/blessed";
import type { Model, Placement, ScanResult } from "~/ipc/contract";

const scan = fixtureScan();
const machine = fixtureMachine();
const plan = derivePlan(scan, machine);

describe("the plan matches the blessed mock", () => {
  it("counts the same models and files", () => {
    expect(plan.models.length).toBe(BLESSED.models);
    expect(plan.totals.files).toBe(BLESSED.files);
  });

  it("counts the same bytes", () => {
    expect(plan.totals.uniqueBytes).toBe(BLESSED.uniqueMB * MB);
    expect(plan.totals.onDiskBytes).toBe(BLESSED.onDiskMB * MB);
    expect(plan.totals.reclaimBytes).toBe(BLESSED.reclaimMB * MB);
    expect(plan.totals.blockedBytes).toBe(BLESSED.blockedMB * MB);
    expect(plan.totals.vaultOnlyBytes).toBe(BLESSED.vaultOnlyMB * MB);
    expect(plan.totals.countedNeverMovedBytes).toBe(
      BLESSED.countedNeverMovedMB * MB,
    );
  });

  it("groups the same way", () => {
    expect(plan.duplicates.length).toBe(BLESSED.duplicateGroups);
    expect(plan.totals.duplicateCopies).toBe(BLESSED.duplicateCopies);
    expect(plan.clashes.length).toBe(BLESSED.clashGroups);
    expect(plan.singles.length).toBe(BLESSED.singles);
    expect(plan.singles.reduce((s, m) => s + m.bytes, 0)).toBe(
      BLESSED.singlesMB * MB,
    );
    expect(plan.blocked.length).toBe(BLESSED.blocked);
    expect(plan.orphans.length).toBe(BLESSED.orphans);
    expect(plan.aliases.length).toBe(BLESSED.aliasGroups);
    expect(plan.totals.unused).toBe(BLESSED.unused);
    expect(plan.folders).toEqual(BLESSED.folders);
  });

  it("gives each install the same figures", () => {
    for (const [id, expected] of Object.entries(BLESSED.perInstance)) {
      const totals = plan.totals.perInstance.get(id)!;
      expect(totals.bytes, id).toBe(expected.mb * MB);
      expect(totals.files, id).toBe(expected.files);
      expect(totals.moving, id).toBe(expected.moving);
      expect(totals.movingBytes, id).toBe(expected.movingMB * MB);
      expect(totals.stuck, id).toBe(expected.stuck);
      expect(totals.stuckBytes, id).toBe(expected.stuckMB * MB);
    }
  });

  it("picks the same ten biggest wins, kept in the same install", () => {
    const top = plan.duplicates.slice(0, 10).map((m) => ({
      filename: m.filename,
      reclaimMB: m.reclaimBytes / MB,
      keeper: m.keeper!.instanceId,
      folder: m.keeper!.folder,
    }));
    expect(top).toEqual(BLESSED.topDuplicates.map((t) => ({ ...t })));
  });

  it("renames the same clashing files in the vault", () => {
    const clashes = plan.clashes.map((group) => ({
      filename: group.filename,
      vaultNames: group.models.map((m) => m.vaultName),
      mb: group.models.map((m) => m.bytes / MB),
    }));
    expect(clashes).toEqual(BLESSED.clashNames.map((c) => ({
      filename: c.filename,
      vaultNames: [...c.vaultNames],
      mb: [...c.mb],
    })));
  });

  it("lists what cannot move in the same order, with the same reasons", () => {
    const blocked = plan.blocked.map((entry) => ({
      filename: entry.model.filename,
      kind: entry.reason.kind,
      mb: entry.model.bytes / MB,
    }));
    expect(blocked).toEqual(BLESSED.blockedOrder.map((b) => ({ ...b })));
  });

  it("says what closing ComfyUI is worth", () => {
    expect(reclaimIfComfyClosed(scan, machine)).toBe(
      (BLESSED.reclaimMB + BLESSED.comfyCostMB) * MB,
    );
  });
});

// ── the rules, on data small enough to read ────────────────────────────────

const VAULT = "C:\\ComfyVault";

function placement(
  id: string,
  instanceId: string,
  folder: string,
  filename: string,
  blocked: Placement["blocked"] = null,
): Placement {
  return {
    id,
    instanceId,
    folder,
    filename,
    fullPath: `C:\\${instanceId}\\${folder}${filename}`,
    isLink: false,
    blocked,
  };
}

function model(
  id: string,
  filename: string,
  bytes: number,
  placements: Placement[],
  extra: Partial<Model> = {},
): Model {
  return {
    id,
    sha256: id.padEnd(64, "0"),
    filename,
    folder: "checkpoints",
    bytes,
    placements,
    workflowHits: 0,
    workflowHitsByName: { [filename]: 0 },
    civitai: null,
    inVaultSince: null,
    ...extra,
  };
}

function scanOf(models: Model[], instanceIds = ["a", "b", "c"]): ScanResult {
  return {
    scannedAt: "2026-09-22T12:00:00.000Z",
    instances: instanceIds.map((id) => ({
      id,
      name: id.toUpperCase(),
      path: `C:\\${id}`,
      running: false,
      extraModelPaths: null,
      addedAt: "2026-09-01T00:00:00.000Z",
    })),
    models,
    countedNeverMoved: [],
    removedInstance: null,
    activity: [],
    civitaiEnabled: false,
  };
}

const machineOf = () =>
  fixtureMachine({
    vaultPath: VAULT,
    running: [],
    developerMode: true,
  });

describe("which copy is kept", () => {
  it("keeps the copy in the install registered first", () => {
    const m = model("m1", "x.safetensors", 100, [
      placement("p-b", "b", "models\\", "x.safetensors"),
      placement("p-a", "a", "models\\", "x.safetensors"),
    ]);
    const derived = derivePlan(scanOf([m]), machineOf());
    expect(derived.models[0]!.keeper!.instanceId).toBe("a");
    expect(derived.models[0]!.liveExtras.map((p) => p.instanceId)).toEqual(["b"]);
  });

  it("skips a copy that cannot move and keeps the next one", () => {
    const m = model("m1", "x.safetensors", 100, [
      placement("p-a", "a", "models\\", "x.safetensors", {
        kind: "permission_denied",
      }),
      placement("p-b", "b", "models\\", "x.safetensors"),
      placement("p-c", "c", "models\\", "x.safetensors"),
    ]);
    const derived = derivePlan(scanOf([m]), machineOf()).models[0]!;
    expect(derived.keeper!.instanceId).toBe("b");
    expect(derived.blocked).toHaveLength(1);
    expect(derived.reclaimBytes).toBe(100);
  });

  it("keeps nothing when every copy is blocked", () => {
    const m = model("m1", "x.safetensors", 100, [
      placement("p-a", "a", "models\\", "x.safetensors", { kind: "permission_denied" }),
    ]);
    const derived = derivePlan(scanOf([m]), machineOf()).models[0]!;
    expect(derived.keeper).toBeNull();
    expect(derived.live).toHaveLength(0);
    expect(derived.reclaimBytes).toBe(0);
  });
});

describe("what a run gives back", () => {
  it("counts one file's size for every copy that becomes a link", () => {
    const m = model("m1", "x.safetensors", 1000, [
      placement("p-a", "a", "models\\", "x.safetensors"),
      placement("p-b", "b", "models\\", "x.safetensors"),
      placement("p-c", "c", "models\\", "x.safetensors"),
    ]);
    const derived = derivePlan(scanOf([m]), machineOf()).models[0]!;
    expect(derived.reclaimBytes).toBe(2000);
  });

  it("gives back nothing for a blocked copy", () => {
    const m = model("m1", "x.safetensors", 1000, [
      placement("p-a", "a", "models\\", "x.safetensors"),
      placement("p-b", "b", "models\\", "x.safetensors", {
        kind: "other_drive",
        drive: "D:",
        vaultDrive: "C:",
      }),
    ]);
    const plan = derivePlan(scanOf([m]), machineOf());
    expect(plan.totals.reclaimBytes).toBe(0);
    expect(plan.duplicates).toHaveLength(0);
    // It still moves into the vault, so it belongs in the third group.
    expect(plan.singles.map((s) => s.id)).toEqual(["m1"]);
  });

  it("counts a file the vault already holds once, not twice", () => {
    const orphan = model("m1", "x.safetensors", 500, []);
    const plan = derivePlan(scanOf([orphan]), machineOf());
    expect(plan.totals.onDiskBytes).toBe(500);
    expect(plan.totals.vaultOnlyBytes).toBe(500);
    expect(plan.orphans.map((o) => o.id)).toEqual(["m1"]);
  });
});

describe("two different files with the same name", () => {
  it("gives the plain name to the largest and numbers the rest", () => {
    const models = [
      model("small", "clash.safetensors", 100, [
        placement("p1", "a", "models\\", "clash.safetensors"),
      ]),
      model("big", "clash.safetensors", 900, [
        placement("p2", "b", "models\\", "clash.safetensors"),
      ]),
      model("middle", "clash.safetensors", 500, [
        placement("p3", "c", "models\\", "clash.safetensors"),
      ]),
    ];
    const plan = derivePlan(scanOf(models), machineOf());
    expect(plan.clashes).toHaveLength(1);
    expect(plan.clashes[0]!.models.map((m) => m.vaultName)).toEqual([
      "clash.safetensors",
      "clash-2.safetensors",
      "clash-3.safetensors",
    ]);
    expect(plan.byId.get("big")!.vaultPath).toBe(
      "C:\\ComfyVault\\checkpoints\\clash.safetensors",
    );
  });

  it("settles files of the same size the same way on every run", () => {
    const build = () =>
      derivePlan(
        scanOf([
          model("zzz", "same.safetensors", 100, [
            placement("p1", "a", "models\\", "same.safetensors"),
          ]),
          model("aaa", "same.safetensors", 100, [
            placement("p2", "b", "models\\", "same.safetensors"),
          ]),
        ]),
        machineOf(),
      );
    const first = build().clashes[0]!.models.map((m) => m.id);
    const second = build().clashes[0]!.models.map((m) => m.id);
    expect(first).toEqual(second);
    expect(first[0]).toBe("aaa");
  });

  it("leaves a name alone when only one file carries it", () => {
    const plan = derivePlan(
      scanOf([
        model("m1", "alone.safetensors", 100, [
          placement("p1", "a", "models\\", "alone.safetensors"),
        ]),
      ]),
      machineOf(),
    );
    expect(plan.clashes).toHaveLength(0);
    expect(plan.models[0]!.vaultName).toBe("alone.safetensors");
  });
});

describe("the same bytes under more than one name", () => {
  it("lists every name, the vault name first", () => {
    const m = model("m1", "canonical.safetensors", 100, [
      placement("p1", "a", "models\\", "canonical.safetensors"),
      placement("p2", "b", "models\\", "other-name.safetensors"),
      placement("p3", "c", "models\\", "other-name.safetensors"),
    ]);
    const derived = derivePlan(scanOf([m]), machineOf()).models[0]!;
    expect(derived.allNames).toEqual([
      "canonical.safetensors",
      "other-name.safetensors",
    ]);
    expect(derived.altNames).toEqual(["other-name.safetensors"]);
  });

  it("does not call a model an alias group when every name matches", () => {
    const m = model("m1", "x.safetensors", 100, [
      placement("p1", "a", "models\\", "x.safetensors"),
      placement("p2", "b", "models\\", "x.safetensors"),
    ]);
    expect(derivePlan(scanOf([m]), machineOf()).aliases).toHaveLength(0);
  });
});

describe("a path that already holds a link", () => {
  const linked = (p: Placement): Placement => ({ ...p, isLink: true });

  it("takes no room, so it is not counted as a duplicate", () => {
    const m = model(
      "m1",
      "x.safetensors",
      1000,
      [
        linked(placement("p-a", "a", "models\\", "x.safetensors")),
        linked(placement("p-b", "b", "models\\", "x.safetensors")),
      ],
      { inVaultSince: "2026-09-20T00:00:00.000Z" },
    );
    const plan = derivePlan(scanOf([m]), machineOf());
    expect(plan.totals.reclaimBytes).toBe(0);
    expect(plan.totals.duplicateCopies).toBe(0);
    expect(plan.duplicates).toHaveLength(0);
    // One real file exists: the vault's own copy.
    expect(plan.totals.onDiskBytes).toBe(1000);
  });

  it("leaves a model with nothing left to do out of every group", () => {
    const m = model(
      "m1",
      "x.safetensors",
      1000,
      [linked(placement("p-a", "a", "models\\", "x.safetensors"))],
      { inVaultSince: "2026-09-20T00:00:00.000Z" },
    );
    const plan = derivePlan(scanOf([m]), machineOf());
    expect(plan.models[0]!.needsWork).toBe(false);
    expect(plan.duplicates).toHaveLength(0);
    expect(plan.singles).toHaveLength(0);
  });

  it("still counts a real copy beside a link", () => {
    const m = model(
      "m1",
      "x.safetensors",
      1000,
      [
        linked(placement("p-a", "a", "models\\", "x.safetensors")),
        placement("p-b", "b", "models\\", "x.safetensors"),
      ],
      { inVaultSince: "2026-09-20T00:00:00.000Z" },
    );
    const derived = derivePlan(scanOf([m]), machineOf());
    // The vault copy plus the one real file that is still out there.
    expect(derived.totals.onDiskBytes).toBe(2000);
    expect(derived.models[0]!.needsWork).toBe(true);
    // The real file is the one that can be kept, not the link.
    expect(derived.models[0]!.keeper!.id).toBe("p-b");
    expect(derived.totals.reclaimBytes).toBe(0);
  });
});

describe("closing ComfyUI changes the plan, not just a flag", () => {
  const openReason = {
    kind: "file_open" as const,
    process: "python.exe",
    pid: 42,
    instanceId: "a",
  };

  it("makes a held-open copy the kept copy once it is free", () => {
    const m = model("m1", "x.safetensors", 1000, [
      placement("p-a", "a", "models\\", "x.safetensors", openReason),
      placement("p-b", "b", "models\\", "x.safetensors"),
    ]);
    const held = derivePlan(scanOf([m]), machineOf());
    expect(held.models[0]!.keeper!.instanceId).toBe("b");
    expect(held.totals.reclaimBytes).toBe(0);

    const free = derivePlan(scanOf([m]), machineOf(), { ignoreOpenFiles: true });
    expect(free.models[0]!.keeper!.instanceId).toBe("a");
    expect(free.totals.reclaimBytes).toBe(1000);
    expect(free.blocked).toHaveLength(0);
  });

  it("leaves the other reasons alone", () => {
    const m = model("m1", "x.safetensors", 1000, [
      placement("p-a", "a", "models\\", "x.safetensors"),
      placement("p-b", "b", "models\\", "x.safetensors", {
        kind: "other_drive",
        drive: "D:",
        vaultDrive: "C:",
      }),
    ]);
    const free = derivePlan(scanOf([m]), machineOf(), { ignoreOpenFiles: true });
    expect(free.blocked).toHaveLength(1);
    expect(free.totals.reclaimBytes).toBe(0);
  });
});
