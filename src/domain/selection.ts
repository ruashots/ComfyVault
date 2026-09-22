/**
 * What the commit bar counts, and the one rule that decides whether Apply can
 * run at all.
 *
 * The person ticks plan groups. Apply is sent exactly the group identifiers they
 * left ticked: the engine never widens the work, so neither does the interface.
 */

import type {
  AppState,
  ConsolidationPlan,
  InterruptedApply,
  PlanGroup,
  PlatformReport,
  RunningComfy,
} from "~/ipc/contract";

export interface Selection {
  /** Space the run returns to the drive. */
  readonly bytes: number;
  /** Groups that give space back. */
  readonly groups: number;
  /** Files that move into the vault, one per group. */
  readonly moves: number;
  /** Links that go back where files were, one per path the group covers. */
  readonly links: number;
  /** Second copies that stop taking room. */
  readonly duplicateCopies: number;
  /** What Apply is sent. */
  readonly groupIds: readonly string[];
}

export const EMPTY_SELECTION: Selection = {
  bytes: 0,
  groups: 0,
  moves: 0,
  links: 0,
  duplicateCopies: 0,
  groupIds: [],
};

/** A group is in the run unless the person unticked it. */
export function isTicked(unticked: ReadonlySet<string>, groupId: string): boolean {
  return !unticked.has(groupId);
}

/**
 * Every path a group covers ends up holding a link, the one the file moved out
 * of included. That is the whole promise: wherever a file was, a link takes its
 * place.
 */
export function linksOf(group: PlanGroup): number {
  return group.occurrences;
}

export function selectionFor(
  plan: ConsolidationPlan | null,
  unticked: ReadonlySet<string>,
): Selection {
  if (!plan) return EMPTY_SELECTION;

  let bytes = 0;
  let groups = 0;
  let moves = 0;
  let links = 0;
  let duplicateCopies = 0;
  const groupIds: string[] = [];

  for (const group of plan.groups) {
    if (!isTicked(unticked, group.groupId)) continue;
    moves += 1;
    links += linksOf(group);
    groupIds.push(group.groupId);
    if (group.bytesFreed > 0) {
      bytes += group.bytesFreed;
      groups += 1;
      duplicateCopies += Math.max(0, group.occurrences - 1);
    }
  }

  return { bytes, groups, moves, links, duplicateCopies, groupIds };
}

// ── the Apply gate ──────────────────────────────────────────────────────────

export type ApplyBlocker =
  | { kind: "symlinks_unsupported"; guidance: string | null; probeError: string | null }
  | { kind: "comfy_running"; processes: readonly RunningComfy[] }
  | { kind: "interrupted_apply"; applies: readonly InterruptedApply[] };

export interface MachineFacts {
  platform: PlatformReport | null;
  running: readonly RunningComfy[];
  interrupted: readonly InterruptedApply[];
}

/**
 * What the machine says has to be fixed before anything can move, in the order
 * the Consolidate screen lists them.
 *
 * A run that was interrupted comes first: the contract says it must be resolved
 * before a new scan or a new apply. Links being unavailable comes next, because
 * without them nothing can move at all.
 */
export function applyBlockers(machine: MachineFacts): ApplyBlocker[] {
  const out: ApplyBlocker[] = [];
  if (machine.interrupted.length > 0) {
    out.push({ kind: "interrupted_apply", applies: machine.interrupted });
  }
  if (machine.platform && !machine.platform.symlinks.supported) {
    out.push({
      kind: "symlinks_unsupported",
      guidance: machine.platform.symlinks.guidance,
      probeError: machine.platform.symlinks.probeError,
    });
  }
  if (machine.running.length > 0) {
    out.push({ kind: "comfy_running", processes: machine.running });
  }
  return out;
}

export type ApplyGate =
  | {
      readonly can: false;
      readonly reason: "blocked";
      readonly blockers: readonly ApplyBlocker[];
    }
  | { readonly can: false; readonly reason: "nothing_ticked" }
  | { readonly can: false; readonly reason: "busy"; readonly what: "scan" | "apply" | "revert" }
  | { readonly can: true };

/** What the gate is holding back on, or nothing. */
export function gateBlockers(gate: ApplyGate): readonly ApplyBlocker[] {
  return gate.can === false && gate.reason === "blocked" ? gate.blockers : [];
}

/** Apply runs only when the machine allows it and something is ticked. */
export function applyGate(
  machine: MachineFacts,
  selection: Selection,
  busy: AppState["busy"] = null,
): ApplyGate {
  if (busy) return { can: false, reason: "busy", what: busy.kind };
  const blockers = applyBlockers(machine);
  if (blockers.length > 0) return { can: false, reason: "blocked", blockers };
  if (selection.moves === 0) return { can: false, reason: "nothing_ticked" };
  return { can: true };
}
