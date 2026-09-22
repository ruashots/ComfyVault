import { describe, expect, it } from "vitest";

import { derivePlan } from "~/domain/plan";
import {
  applyBlockers,
  applyGate,
  gateBlockers,
  selectionFor,
} from "~/domain/selection";
import { fixtureMachine, fixtureScan } from "~/ipc/fixture/dataset";
import { BLESSED, MB } from "~/test/blessed";

const scan = fixtureScan();
const machine = fixtureMachine();
const plan = derivePlan(scan, machine);
const none = new Set<string>();

describe("the commit bar with everything ticked", () => {
  it("matches the blessed mock", () => {
    const selection = selectionFor(plan, none);
    expect(selection.bytes).toBe(BLESSED.selectionAll.mb * MB);
    expect(selection.groups).toBe(BLESSED.selectionAll.groups);
    expect(selection.moves).toBe(BLESSED.selectionAll.moves);
    expect(selection.links).toBe(BLESSED.selectionAll.links);
    expect(selection.duplicateCopies).toBe(BLESSED.selectionAll.duplicateCopies);
  });

  it("moves one file per model that has something to move", () => {
    const selection = selectionFor(plan, none);
    expect(selection.moves).toBe(plan.models.length - plan.orphans.length);
    expect(selection.models).toHaveLength(selection.moves);
  });

  it("creates one link for every copy that can move", () => {
    const selection = selectionFor(plan, none);
    const movable = plan.models.reduce((sum, m) => sum + m.live.length, 0);
    expect(selection.links).toBe(movable);
    expect(selection.links).toBe(plan.totals.files - plan.blocked.length);
  });

  it("counts each model once, even one that both clashes and duplicates", () => {
    const ids = selectionFor(plan, none).models.map((m) => m.id);
    expect(new Set(ids).size).toBe(ids.length);
  });
});

describe("ticking and unticking", () => {
  it("takes the biggest win out of the total when it is unticked", () => {
    const biggest = plan.duplicates[0]!;
    const all = selectionFor(plan, none);
    const without = selectionFor(plan, new Set([biggest.id]));
    expect(all.bytes - without.bytes).toBe(biggest.reclaimBytes);
    expect(all.moves - without.moves).toBe(1);
    expect(all.links - without.links).toBe(biggest.live.length);
    expect(all.groups - without.groups).toBe(1);
  });

  it("drops the totals to nothing when everything is unticked", () => {
    const everything = new Set(plan.models.map((m) => m.id));
    const selection = selectionFor(plan, everything);
    expect(selection).toMatchObject({
      bytes: 0,
      groups: 0,
      moves: 0,
      links: 0,
      duplicateCopies: 0,
    });
    expect(selection.models).toHaveLength(0);
  });

  it("changes nothing when a file with nothing to move is unticked", () => {
    const orphan = plan.orphans[0]!;
    expect(selectionFor(plan, new Set([orphan.id]))).toEqual(
      selectionFor(plan, none),
    );
  });

  it("never counts a file the vault already holds alone", () => {
    const ids = selectionFor(plan, none).models.map((m) => m.id);
    for (const orphan of plan.orphans) expect(ids).not.toContain(orphan.id);
  });

  it("leaves a model out when every one of its copies is blocked", () => {
    const stuck = plan.models.filter((m) => m.live.length === 0 && !m.isOrphan);
    const ids = selectionFor(plan, none).models.map((m) => m.id);
    for (const model of stuck) expect(ids).not.toContain(model.id);
  });
});

describe("a model whose copies are already links", () => {
  it("is left out of the run, so Apply has nothing to repeat", () => {
    const applied = derivePlan(
      {
        ...scan,
        models: scan.models.map((m) => ({
          ...m,
          inVaultSince: "2026-09-20T00:00:00.000Z",
          placements: m.placements.map((p) =>
            p.blocked === null ? { ...p, isLink: true } : p,
          ),
        })),
      },
      machine,
    );
    const selection = selectionFor(applied, none);
    expect(selection.bytes).toBe(0);
    expect(selection.moves).toBe(0);
    expect(selection.models).toHaveLength(0);
  });
});

describe("Apply is held back until the machine allows it", () => {
  it("names both reasons on the person's machine as it is now", () => {
    const blockers = applyBlockers(machine);
    expect(blockers.map((b) => b.kind)).toEqual([
      "developer_mode_off",
      "comfy_running",
    ]);
  });

  it("refuses to run while Developer Mode is off", () => {
    const gate = applyGate(
      fixtureMachine({ developerMode: false, running: [] }),
      selectionFor(plan, none),
    );
    expect(gate.can).toBe(false);
    expect(gateBlockers(gate).map((b) => b.kind)).toEqual(["developer_mode_off"]);
  });

  it("refuses to run while a ComfyUI is running", () => {
    const gate = applyGate(
      fixtureMachine({ developerMode: true }),
      selectionFor(plan, none),
    );
    expect(gate.can).toBe(false);
    expect(gateBlockers(gate).map((b) => b.kind)).toEqual(["comfy_running"]);
  });

  it("refuses to run when nothing is ticked, even with a clear machine", () => {
    const everything = new Set(plan.models.map((m) => m.id));
    const gate = applyGate(
      fixtureMachine({ developerMode: true, running: [] }),
      selectionFor(plan, everything),
    );
    expect(gate).toEqual({ can: false, reason: "nothing_ticked" });
  });

  it("runs once the machine is clear and something is ticked", () => {
    const gate = applyGate(
      fixtureMachine({ developerMode: true, running: [] }),
      selectionFor(plan, none),
    );
    expect(gate).toEqual({ can: true });
  });

  it("reports the machine before it reports an empty selection", () => {
    // A blocked machine is what the person has to fix first, so that is what the
    // button says, whether or not anything is ticked.
    const everything = new Set(plan.models.map((m) => m.id));
    const gate = applyGate(machine, selectionFor(plan, everything));
    expect(gate.can).toBe(false);
    expect(gate.can === false && gate.reason).toBe("blocked");
  });
});
