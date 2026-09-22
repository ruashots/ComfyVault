/**
 * What the commit bar counts, and the one rule that decides whether Apply can
 * run at all.
 *
 * A model joins the run when the person leaves it ticked AND at least one of its
 * copies can move. Each model is counted once, whether it appears in the
 * duplicates list, the filename-clash list or the moves-but-frees-nothing list.
 */

import type { ComfyProcess, MachineState } from "~/ipc/contract";
import type { Plan, PlannedModel } from "~/domain/plan";

export interface Selection {
  /** Space the run returns to the drive. */
  readonly bytes: number;
  /** Models held twice or more that are ticked. */
  readonly groups: number;
  /** Files that move into the vault, one per model. */
  readonly moves: number;
  /** Links that go back where files were, one per copy that can move. */
  readonly links: number;
  /** Second copies that stop taking room. */
  readonly duplicateCopies: number;
  /** The models the run will act on, in plan order. */
  readonly models: readonly PlannedModel[];
}

export const EMPTY_SELECTION: Selection = {
  bytes: 0,
  groups: 0,
  moves: 0,
  links: 0,
  duplicateCopies: 0,
  models: [],
};

/** A model is in the run unless the person unticked it. */
export function isTicked(unticked: ReadonlySet<string>, id: string): boolean {
  return !unticked.has(id);
}

export function selectionFor(
  plan: Plan,
  unticked: ReadonlySet<string>,
): Selection {
  let bytes = 0;
  let groups = 0;
  let moves = 0;
  let links = 0;
  let duplicateCopies = 0;
  const models: PlannedModel[] = [];

  for (const model of plan.models) {
    if (!isTicked(unticked, model.id)) continue;
    // Nothing to do for a model whose every copy is already a link.
    if (!model.needsWork) continue;

    moves += 1;
    links += model.live.length;
    models.push(model);

    if (model.reclaimableExtras.length > 0) {
      bytes += model.reclaimBytes;
      groups += 1;
      duplicateCopies += model.reclaimableExtras.length;
    }
  }

  return { bytes, groups, moves, links, duplicateCopies, models };
}

// ── the Apply gate ──────────────────────────────────────────────────────────

export type ApplyBlocker =
  | { kind: "developer_mode_off" }
  | { kind: "comfy_running"; processes: readonly ComfyProcess[] };

/**
 * What the machine says has to be fixed before anything can move. Order is the
 * order the Consolidate screen lists them in.
 */
export function applyBlockers(machine: MachineState): ApplyBlocker[] {
  const out: ApplyBlocker[] = [];
  if (!machine.developerMode) out.push({ kind: "developer_mode_off" });
  if (machine.running.length > 0) {
    out.push({ kind: "comfy_running", processes: machine.running });
  }
  return out;
}

export type ApplyGate =
  | { readonly can: false; readonly reason: "blocked"; readonly blockers: readonly ApplyBlocker[] }
  | { readonly can: false; readonly reason: "nothing_ticked" }
  | { readonly can: true };

/** What the gate is holding back on, or nothing. */
export function gateBlockers(gate: ApplyGate): readonly ApplyBlocker[] {
  return gate.can === false && gate.reason === "blocked" ? gate.blockers : [];
}

/** Apply runs only when the machine allows it and something is ticked. */
export function applyGate(
  machine: MachineState,
  selection: Selection,
): ApplyGate {
  const blockers = applyBlockers(machine);
  if (blockers.length > 0) return { can: false, reason: "blocked", blockers };
  if (selection.moves === 0) return { can: false, reason: "nothing_ticked" };
  return { can: true };
}
