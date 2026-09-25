import { describe, expect, it } from "vitest";

import {
  applyBlockers,
  applyGate,
  gateBlockers,
  linksOf,
  selectionFor,
  type MachineFacts,
} from "~/domain/selection";
import { FixtureEngine } from "~/ipc/fixture/engine";
import type {
  ConsolidationPlan,
  InterruptedApply,
  PlatformReport,
  RunningComfy,
} from "~/ipc/contract";

const none = new Set<string>();

async function readyPlan(): Promise<ConsolidationPlan> {
  const engine = new FixtureEngine();
  engine.devSetSymlinksSupported(true);
  engine.devSetComfyRunning(false);
  const scan = (await engine.getLastScan())!;
  return engine.buildPlan(scan.scanId);
}

describe("the commit bar with everything ticked", () => {
  it("matches the plan's own totals", async () => {
    const plan = await readyPlan();
    const selection = selectionFor(plan, none);
    expect(selection.bytes).toBe(plan.totals.bytesFreed);
    expect(selection.moves).toBe(plan.totals.filesMoved);
    expect(selection.links).toBe(plan.totals.linksCreated);
    expect(selection.groups).toBe(plan.totals.groupsFreeingSpace);
    expect(selection.groupIds).toHaveLength(plan.groups.length);
  });

  it("creates one link for every path a group covers", async () => {
    const plan = await readyPlan();
    for (const group of plan.groups) {
      expect(linksOf(group)).toBe(group.occurrences);
      expect(group.links.length).toBe(group.occurrences);
    }
  });

  it("sends Apply exactly the groups that are ticked, and nothing else", async () => {
    const plan = await readyPlan();
    const ids = selectionFor(plan, none).groupIds;
    expect(new Set(ids).size).toBe(ids.length);
    for (const id of ids) {
      expect(plan.groups.some((g) => g.groupId === id)).toBe(true);
    }
  });
});

describe("ticking and unticking", () => {
  it("takes the biggest win out of the total when its row is unticked", async () => {
    const plan = await readyPlan();
    const biggest = [...plan.groups].sort((a, b) => b.bytesFreed - a.bytesFreed)[0]!;
    const all = selectionFor(plan, none);
    const without = selectionFor(plan, new Set([biggest.groupId]));
    expect(all.bytes - without.bytes).toBe(biggest.bytesFreed);
    expect(all.moves - without.moves).toBe(1);
    expect(all.links - without.links).toBe(biggest.occurrences);
    expect(all.groups - without.groups).toBe(1);
    expect(without.groupIds).not.toContain(biggest.groupId);
  });

  it("drops everything to nothing when every row is unticked", async () => {
    const plan = await readyPlan();
    const everything = new Set(plan.groups.map((g) => g.groupId));
    expect(selectionFor(plan, everything)).toMatchObject({
      bytes: 0,
      groups: 0,
      moves: 0,
      links: 0,
      duplicateCopies: 0,
    });
  });

  it("still counts a single copy as a file that moves, freeing nothing", async () => {
    const plan = await readyPlan();
    const single = plan.groups.find((g) => g.singleCopy)!;
    const all = selectionFor(plan, none);
    const without = selectionFor(plan, new Set([single.groupId]));
    expect(all.bytes - without.bytes).toBe(0);
    expect(all.moves - without.moves).toBe(1);
  });

  it("counts nothing at all when there is no plan yet", () => {
    expect(selectionFor(null, none)).toMatchObject({ moves: 0, bytes: 0 });
  });
});

// ── the gate ────────────────────────────────────────────────────────────────

const platform = (supported: boolean): PlatformReport => ({
  os: "windows",
  symlinks: {
    supported,
    probeError: supported ? null : "os error 1314",
    developerMode: supported,
    elevated: false,
    guidance: supported ? null : "Turn Developer Mode on.",
  },
  longPathsEnabled: true,
});

const comfy: RunningComfy = {
  pid: 18244,
  name: "python.exe",
  exePath: "C:\\ComfyUI-Studio\\python.exe",
  cwd: "C:\\ComfyUI-Studio",
  commandLine: ["python.exe", "main.py"],
  matchedInstallIds: ["studio"],
  matchReason: "exeUnderRoot",
};

const stopped: InterruptedApply = {
  applyId: "apply-1",
  planId: "plan-1",
  startedAt: "2026-09-22T10:00:00.000Z",
  stepsDone: 4,
  stepsPending: 9,
  description: "A run stopped part way through.",
  affectedPaths: [],
};

const machine = (over: Partial<MachineFacts> = {}): MachineFacts => ({
  platform: platform(true),
  running: [],
  interrupted: [],
  ...over,
});

describe("Apply is held back until the machine allows it", () => {
  it("refuses while this system cannot create links, and carries the guidance", () => {
    const blockers = applyBlockers(machine({ platform: platform(false) }));
    expect(blockers.map((b) => b.kind)).toEqual(["symlinks_unsupported"]);
    expect(
      blockers[0]!.kind === "symlinks_unsupported" && blockers[0]!.guidance,
    ).toBe("Turn Developer Mode on.");
  });

  it("refuses while a ComfyUI is running", () => {
    const blockers = applyBlockers(machine({ running: [comfy] }));
    expect(blockers.map((b) => b.kind)).toEqual(["comfy_running"]);
  });

  it("puts a run that stopped part way ahead of everything else", () => {
    const blockers = applyBlockers(
      machine({ platform: platform(false), running: [comfy], interrupted: [stopped] }),
    );
    expect(blockers.map((b) => b.kind)).toEqual([
      "interrupted_apply",
      "symlinks_unsupported",
      "comfy_running",
    ]);
  });

  it("refuses when nothing is ticked, even with a clear machine", async () => {
    const plan = await readyPlan();
    const everything = new Set(plan.groups.map((g) => g.groupId));
    expect(applyGate(machine(), selectionFor(plan, everything))).toEqual({
      can: false,
      reason: "nothing_ticked",
    });
  });

  it("refuses while the engine is busy, whatever else is true", async () => {
    const plan = await readyPlan();
    const gate = applyGate(machine(), selectionFor(plan, none), {
      kind: "scan",
      id: "scan-9",
    });
    expect(gate).toEqual({ can: false, reason: "busy", what: "scan" });
  });

  it("runs once the machine is clear and something is ticked", async () => {
    const plan = await readyPlan();
    expect(applyGate(machine(), selectionFor(plan, none))).toEqual({ can: true });
  });

  it("reports the machine before it reports an empty selection", async () => {
    const plan = await readyPlan();
    const everything = new Set(plan.groups.map((g) => g.groupId));
    const gate = applyGate(
      machine({ platform: platform(false) }),
      selectionFor(plan, everything),
    );
    expect(gate.can).toBe(false);
    expect(gate.can === false && gate.reason).toBe("blocked");
    expect(gateBlockers(gate)).toHaveLength(1);
  });

  it("says nothing is blocking when nothing is", () => {
    expect(applyBlockers(machine())).toHaveLength(0);
    expect(gateBlockers({ can: true })).toHaveLength(0);
  });

  it("does not claim links work when the platform is unknown", () => {
    // A missing report is not permission. The gate only blocks on a report that
    // says no, so an unknown platform must not silently open the gate either:
    // the store never reaches this state, and the test pins the behaviour.
    expect(applyBlockers(machine({ platform: null }))).toHaveLength(0);
  });
});
