/**
 * Turn what the engine returns into what the screens draw.
 *
 * The engine owns the plan. This file never decides which copy is kept or what
 * a clashing file is renamed to: it reads those decisions out of the plan and
 * arranges them. What it does own is the arranging, and it is pure, so every
 * rule below is testable on its own.
 */

import { blockedRank, isSkippedByDesign } from "~/domain/blocked";
import type {
  BlockedRow,
  ConsolidationPlan,
  Install,
  LinkRecord,
  NameGroup,
  PlanGroup,
  ScanTotals,
  UsageResult,
} from "~/ipc/contract";

// ── the Consolidate view ────────────────────────────────────────────────────

export interface ClashView {
  /** The plain name both files want. */
  filename: string;
  /** The group that keeps the plain name first, then the ones that were renamed. */
  groups: readonly PlanGroup[];
}

export interface CountedNeverMoved {
  kind: "custom_nodes" | "huggingface_cache";
  files: number;
  bytes: number;
}

export interface PlanView {
  readonly plan: ConsolidationPlan;
  /** What the listed blocked rows add up to, so the count matches the list. */
  readonly blockedBytes: number;
  /** Groups that give space back, biggest win first. */
  readonly duplicates: readonly PlanGroup[];
  /** Groups that move into the vault and free nothing. */
  readonly singles: readonly PlanGroup[];
  /** Two different files that want one name. */
  readonly clashes: readonly ClashView[];
  /** What cannot move and is worth reading, grouped by reason. */
  readonly blocked: readonly BlockedRow[];
  /** Weights that are counted and never moved. Not a problem, so not listed. */
  readonly countedNeverMoved: readonly CountedNeverMoved[];
  readonly byGroupId: ReadonlyMap<string, PlanGroup>;
}

export function buildPlanView(
  plan: ConsolidationPlan,
  totals: ScanTotals | null,
): PlanView {
  const duplicates = plan.groups
    .filter((g) => g.bytesFreed > 0)
    .sort((a, b) => b.bytesFreed - a.bytesFreed);

  const singles = plan.groups
    .filter((g) => g.singleCopy)
    .sort((a, b) => b.sizeBytes - a.sizeBytes);

  const clashes = buildClashes(plan.groups);

  const blocked = plan.blocked
    .filter((row) => !isSkippedByDesign(row.reason))
    .sort(
      (a, b) =>
        blockedRank(a.reason) - blockedRank(b.reason) ||
        b.sizeBytes - a.sizeBytes,
    );

  const countedNeverMoved: CountedNeverMoved[] = [];
  if (totals && totals.customNodeFiles > 0) {
    countedNeverMoved.push({
      kind: "custom_nodes",
      files: totals.customNodeFiles,
      bytes: totals.customNodeBytes,
    });
  }
  if (totals && totals.hfCacheFiles > 0) {
    countedNeverMoved.push({
      kind: "huggingface_cache",
      files: totals.hfCacheFiles,
      bytes: totals.hfCacheBytes,
    });
  }

  return {
    plan,
    blockedBytes: blocked.reduce((sum, row) => sum + row.sizeBytes, 0),
    duplicates,
    singles,
    clashes,
    blocked,
    countedNeverMoved,
    byGroupId: new Map(plan.groups.map((g) => [g.groupId, g])),
  };
}

/**
 * A renamed group names the hash that took the plain name. Follow that back to
 * pair them up, and put the group that kept the plain name first.
 */
function buildClashes(groups: readonly PlanGroup[]): ClashView[] {
  const bySha = new Map(groups.map((g) => [g.sha256, g]));
  const families = new Map<string, PlanGroup[]>();

  for (const group of groups) {
    if (!group.vaultNameAdjusted || !group.clashesWith) continue;
    const owner = bySha.get(group.clashesWith);
    const key = owner?.sha256 ?? group.clashesWith;
    const family = families.get(key);
    if (family) family.push(group);
    else families.set(key, owner ? [owner, group] : [group]);
  }

  return [...families.values()].map((family) => ({
    filename: fileNameOf(family[0]!.vaultRelPath),
    groups: family,
  }));
}

/** "loras/lora1.safetensors" -> "lora1.safetensors" */
export function fileNameOf(relPath: string): string {
  const at = Math.max(relPath.lastIndexOf("/"), relPath.lastIndexOf("\\"));
  return at < 0 ? relPath : relPath.slice(at + 1);
}

/** Why the engine chose this copy to become the vault file. */
export function chosenBecauseText(
  group: PlanGroup,
  vaultVolume: string,
): string {
  // Nothing is kept: the vault already holds the file, and the engine still
  // names a source, "onlyCopy" when there is one copy, which read as "kept".
  if (group.alreadyInVault) {
    return `the vault already holds this model from an earlier run, so nothing moves in and every copy here becomes a link to it`;
  }
  switch (group.source.chosenBecause) {
    case "sameVolume":
      return `kept the copy in ${group.source.installLabel}, which is already on drive ${vaultVolume}, so moving it is a rename and takes no time`;
    case "onlyCopy":
      return `kept the copy in ${group.source.installLabel}, the only one there is`;
    case "firstByPath":
      return `kept the copy in ${group.source.installLabel}, the first by path, because no copy is on drive ${vaultVolume} yet`;
  }
}

// ── the Library drawer ──────────────────────────────────────────────────────

export type PlaceKind = "source" | "willLink" | "isLink" | "stays";

export interface Place {
  installId: string;
  installLabel: string;
  absPath: string;
  relPath: string;
  /** The name the file carries at this place. */
  name: string;
  kind: PlaceKind;
  blocked: BlockedRow | null;
}

/**
 * Every place one content is reachable from, for the drawer.
 *
 * The list itself comes from the engine one row per content. This fills in the
 * paths for the one row the person opened, from the plan for what is still out
 * in the installs and from the vault's own links for what is already in.
 */
export function placesOf(
  sha256: string,
  plan: ConsolidationPlan | null,
  links: readonly LinkRecord[],
  installLabels: ReadonlyMap<string, string>,
): Place[] {
  const label = (id: string) => installLabels.get(id) ?? id;
  const places: Place[] = links.map((link) => ({
    installId: link.installId,
    installLabel: label(link.installId),
    absPath: link.absPath,
    relPath: link.relPath,
    name: link.linkName,
    kind: "isLink",
    blocked: null,
  }));
  const seen = new Set(places.map((p) => p.absPath.toLowerCase()));

  const group = plan?.groups.find((g) => g.sha256 === sha256);
  if (group) {
    for (const link of group.links) {
      if (seen.has(link.absPath.toLowerCase())) continue;
      seen.add(link.absPath.toLowerCase());
      places.push({
        installId: link.installId,
        installLabel: link.installLabel,
        absPath: link.absPath,
        relPath: link.relPath,
        name: link.linkName,
        kind: link.isSource && !group.alreadyInVault ? "source" : "willLink",
        blocked: null,
      });
    }
  }

  for (const row of plan?.blocked ?? []) {
    if (row.sha256 !== sha256 || isSkippedByDesign(row.reason)) continue;
    if (seen.has(row.absPath.toLowerCase())) continue;
    seen.add(row.absPath.toLowerCase());
    places.push({
      installId: row.installId ?? "",
      installLabel: row.installLabel ?? "",
      absPath: row.absPath,
      relPath: row.absPath,
      name: fileNameOf(row.absPath),
      kind: "stays",
      blocked: row,
    });
  }

  return places;
}

// ── installs ────────────────────────────────────────────────────────────────

export interface InstallView {
  install: Install;
  /** A ComfyUI process is running out of it right now. */
  running: boolean;
  /** Files this install holds, and what the plan does with them. */
  files: number;
  bytes: number;
  moving: number;
  movingBytes: number;
  stuck: number;
  stuckBytes: number;
  /**
   * ComfyUI 0.28.0 and later refuse to serve a preview thumbnail through a
   * per-file link. Loading a model is unaffected either way.
   *
   * "unknown" is its own answer. ComfyUI only began recording its version on
   * disk in 0.3.11, so an older install cannot say, and an install that cannot
   * say is not an install that is known to be fine.
   */
  thumbnails: ThumbnailState;
}

export type ThumbnailState = "affected" | "unaffected" | "unknown";

export function buildInstallViews(
  installs: readonly Install[],
  plan: ConsolidationPlan | null,
  runningInstallIds: ReadonlySet<string>,
): InstallView[] {
  const moving = new Map<string, { files: number; bytes: number }>();
  const stuck = new Map<string, { files: number; bytes: number }>();
  const bump = (
    map: Map<string, { files: number; bytes: number }>,
    id: string | null,
    bytes: number,
  ) => {
    if (!id) return;
    const cur = map.get(id) ?? { files: 0, bytes: 0 };
    map.set(id, { files: cur.files + 1, bytes: cur.bytes + bytes });
  };

  if (plan) {
    // Every path a group covers ends up holding a link, the one the file moved
    // out of included, so the links are the whole list. Counting the source as
    // well would count it twice.
    for (const group of plan.groups) {
      for (const link of group.links) bump(moving, link.installId, group.sizeBytes);
    }
    for (const row of plan.blocked) {
      if (isSkippedByDesign(row.reason)) continue;
      bump(stuck, row.installId, row.sizeBytes);
    }
  }

  return installs.map((install) => {
    const totals = install.lastScanTotals;
    const m = moving.get(install.id) ?? { files: 0, bytes: 0 };
    const s = stuck.get(install.id) ?? { files: 0, bytes: 0 };
    return {
      install,
      running: runningInstallIds.has(install.id),
      files: totals?.movableFiles ?? m.files + s.files,
      bytes: totals?.movableBytes ?? m.bytes + s.bytes,
      moving: m.files,
      movingBytes: m.bytes,
      stuck: s.files,
      stuckBytes: s.bytes,
      thumbnails: thumbnailStateOf(install.version),
    };
  });
}

/** The first ComfyUI that will not serve a thumbnail through a link. */
const THUMBNAILS_LOST_AT: readonly [number, number, number] = [0, 28, 0];

/**
 * Whether this install loses model thumbnails. An install that does not record
 * its version cannot be called fine, so it gets its own answer.
 */
export function thumbnailStateOf(version: string | null): ThumbnailState {
  if (!version) return "unknown";
  const parsed = version.replace(/^v/i, "").split(".");
  if (parsed.some((part) => Number.isNaN(Number.parseInt(part, 10)))) {
    return "unknown";
  }
  return isAtLeast(version, THUMBNAILS_LOST_AT) ? "affected" : "unaffected";
}

/** True when a version string is at least the given release. */
export function isAtLeast(
  version: string | null,
  minimum: readonly [number, number, number],
): boolean {
  if (!version) return false;
  const parts = version
    .replace(/^v/i, "")
    .split(".")
    .map((p) => Number.parseInt(p, 10));
  for (let i = 0; i < 3; i++) {
    const got = parts[i];
    if (got === undefined || Number.isNaN(got)) return false;
    const want = minimum[i]!;
    if (got > want) return true;
    if (got < want) return false;
  }
  return true;
}

// ── cleanup ─────────────────────────────────────────────────────────────────

export interface NameChoice {
  name: string;
  isCanonical: boolean;
  /** Install links that resolve through this name. */
  usedByLinks: number;
  seenInInstalls: readonly string[];
  /** True when nothing on disk uses this name, so removing it is safe. */
  removable: boolean;
}

export interface NameGroupView {
  group: NameGroup;
  choices: readonly NameChoice[];
  /** The name the vault should keep, and why. */
  suggestion: { name: string; reason: string };
}

/**
 * Which name the vault should keep: the one the most install links already
 * resolve through, then the one seen in the most installs, then the longer one.
 * The reason is printed under the choices, so the rule is never a secret.
 */
export function buildNameGroupView(group: NameGroup): NameGroupView {
  const choices: NameChoice[] = group.names.map((n) => ({
    name: n.name,
    isCanonical: n.isCanonical,
    usedByLinks: n.usedByLinks,
    seenInInstalls: n.seenInInstalls,
    removable: n.usedByLinks === 0 && !n.isCanonical,
  }));

  const ranked = [...group.names].sort(
    (a, b) =>
      b.usedByLinks - a.usedByLinks ||
      b.seenInInstalls.length - a.seenInInstalls.length ||
      b.name.length - a.name.length,
  );
  const best = ranked[0]!;
  const rest = ranked.slice(1);

  const linksPoint = (n: number) =>
    `${n} ${n === 1 ? "link points" : "links point"}`;

  let reason: string;
  if (best.usedByLinks > 0) {
    const other = rest[0];
    if (other && other.usedByLinks === best.usedByLinks) {
      reason = `the same number of links point at either, so this is the longer name`;
    } else if (other && other.usedByLinks > 0) {
      reason = `${linksPoint(best.usedByLinks)} at this name already, ${other.usedByLinks} at the next one`;
    } else {
      reason = `${linksPoint(best.usedByLinks)} at this name and none at ${rest.length === 1 ? "the other" : "the others"}`;
    }
  } else if (best.seenInInstalls.length > 0) {
    reason = `this is the name ${best.seenInInstalls.join(" and ")} ${best.seenInInstalls.length === 1 ? "uses" : "use"}`;
  } else {
    reason =
      rest.length === 1
        ? "longer name, and no link points at either"
        : "longest name, and no link points at any of them";
  }

  return { group, choices, suggestion: { name: best.name, reason } };
}

/** The sentence the engine requires next to every used or not-used answer. */
export function usageMethodOf(results: readonly UsageResult[]): string | null {
  return results[0]?.method ?? null;
}
