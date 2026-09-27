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
  UsageMatch,
  UsageResult,
  VaultFile,
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

/** "loras/x.safetensors" -> "loras/" */
export function folderOf(path: string): string {
  const at = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  return at < 0 ? "" : path.slice(0, at + 1);
}

const NUMBER_WORDS = ["no", "one", "two", "three", "four", "five", "six"];

/** 2 -> "two". Past six, the digits read better than the word. */
export function numberWord(n: number): string {
  return NUMBER_WORDS[n] ?? String(n);
}

/** "3 copies of the same 659 MB file", or "1 copy of a 659 MB file". */
export function copiesOf(group: PlanGroup, size: string): string {
  return group.occurrences === 1
    ? `1 copy of a ${size} file`
    : `${group.occurrences} copies of the same ${size} file`;
}

const capital = (s: string) => s.charAt(0).toUpperCase() + s.slice(1);

/** The names the installs use for this model, each once, in engine order. */
export function linkNamesOf(group: PlanGroup): string[] {
  return [...new Set(group.links.map((l) => l.linkName))];
}

/**
 * The name a model is listed under: the vault's name, unless the vault had to
 * add a code to it, which is not a name the person knows.
 */
export function modelTitleOf(group: PlanGroup): string {
  return group.vaultNameAdjusted && group.links[0]
    ? group.links[0].linkName
    : fileNameOf(group.vaultRelPath);
}

/** One sentence under a model, and whether it is the one to notice. */
export interface WhyPart {
  text: string;
  alt: boolean;
}

/**
 * What is worth knowing about one model with more than one copy, one sentence
 * per condition, in a fixed order. Empty when there is nothing to say.
 */
export function duplicateWhy(group: PlanGroup): WhyPart[] {
  const parts: WhyPart[] = [];
  const names = linkNamesOf(group);
  if (names.length > 1) {
    parts.push({ text: `${capital(numberWord(names.length))} names for one model.`, alt: true });
    parts.push({
      text: "Each install keeps the name it uses now, so its workflows still open. After the run, pick one name in Cleanup.",
      alt: false,
    });
  }
  if (group.vaultNameAdjusted) {
    parts.push({
      text: `Another model already has this name, so the vault file carries the start of this one's fingerprint in its name. The installs keep ${names[0] ?? fileNameOf(group.vaultRelPath)}. See Different files with the same name, below.`,
      alt: false,
    });
  }
  const second = group.occurrences - group.distinctFiles;
  if (second > 0) {
    parts.push({
      text:
        second === 1
          ? "One of these copies is a second name for another one above, so it frees no space."
          : `${second} of these copies are second names for others above, so they free no space.`,
      alt: false,
    });
  }
  if (
    !group.alreadyInVault &&
    (group.source.chosenBecause === "firstByPath" || group.crossVolume)
  ) {
    parts.push({
      text: "No copy is on the vault drive, so one copy will be copied across and checked before anything is deleted.",
      alt: false,
    });
  }
  return parts;
}

/** "Both copies will be" or "All 3 copies will be", then where they will point. */
export function duplicateAfter(group: PlanGroup): string {
  const to = group.alreadyInVault
    ? "the file the vault already holds"
    : "one file in the vault";
  // A copy that turned up after an earlier run can be the only one.
  if (group.occurrences === 1) return `The copy will be replaced by a link to ${to}:`;
  const who =
    group.occurrences === 2 ? "Both copies will be" : `All ${group.occurrences} copies will be`;
  return `${who} replaced by links to ${to}:`;
}

/**
 * The code the vault added to a name that was taken, found by comparing the
 * vault's name with the plain one: "model__4898C16F.safetensors" against
 * "model.safetensors" gives "__4898C16F". Null when nothing was added.
 */
export function addedCode(
  vaultName: string,
  plainName: string,
): { before: string; code: string; after: string } | null {
  const dot = plainName.lastIndexOf(".");
  const stem = dot > 0 ? plainName.slice(0, dot) : plainName;
  const ext = dot > 0 ? plainName.slice(dot) : "";
  if (
    vaultName === plainName ||
    !vaultName.startsWith(stem) ||
    !vaultName.endsWith(ext) ||
    vaultName.length <= stem.length + ext.length
  ) {
    return null;
  }
  return {
    before: stem,
    code: vaultName.slice(stem.length, vaultName.length - ext.length),
    after: ext,
  };
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
        installLabel: label(link.installId),
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
      installLabel: row.installId ? label(row.installId) : "",
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
      // A second name for one file frees nothing when it goes, so its bytes
      // do not leave the folder twice.
      for (const link of group.links) {
        bump(moving, link.installId, link.sharesBytesWithAnother ? 0 : group.sizeBytes);
      }
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
  /** The vault file has this name now. The others are links beside it. */
  isCanonical: boolean;
  /** The installs whose links carry this name, each once. */
  installIds: readonly string[];
  /** How many links in the installs carry this name. */
  linksNamed: number;
  /** Nothing reaches the file through this vault name, so it can go. */
  removable: boolean;
}

export interface NameGroupView {
  group: NameGroup;
  choices: readonly NameChoice[];
  /** The name the vault file should carry, and why, in a sentence. */
  suggestion: { name: string; reason: string };
}

/**
 * What each name of one model is, from the links themselves: a link keeps the
 * name its install uses, whatever the vault file is called. The suggestion is
 * the name the most links carry, then the one the most installs use, then the
 * longer one, and the reason is printed, so the rule is never a secret.
 */
export function buildNameGroupView(
  group: NameGroup,
  links: readonly Pick<LinkRecord, "installId" | "linkName">[],
): NameGroupView {
  const choices: NameChoice[] = group.names.map((n) => {
    const named = links.filter((l) => l.linkName === n.name);
    return {
      name: n.name,
      isCanonical: n.isCanonical,
      installIds: [...new Set(named.map((l) => l.installId))],
      linksNamed: named.length,
      // The engine refuses to remove a name a link reaches the file through.
      removable: !n.isCanonical && n.usedByLinks === 0,
    };
  });

  const ranked = [...choices].sort(
    (a, b) =>
      b.linksNamed - a.linksNamed ||
      b.installIds.length - a.installIds.length ||
      b.name.length - a.name.length,
  );
  const best = ranked[0]!;
  const next = ranked[1];
  const links1 = (n: number) => `${n} ${n === 1 ? "link" : "links"}`;

  let reason: string;
  if (best.linksNamed === 0) {
    reason = "No link in the installs has any of these names, so the longer name is suggested.";
  } else if (next && next.linksNamed === best.linksNamed) {
    reason = `As many links in the installs have each name, ${links1(best.linksNamed)} each, so the longer name is suggested.`;
  } else if (next && next.linksNamed > 0) {
    reason = `Suggested because more of the installs' links have this name: ${links1(best.linksNamed)}, against ${next.linksNamed} for the next name.`;
  } else {
    reason = `Suggested because it is the only name the installs' links use: ${links1(best.linksNamed)} have it.`;
  }

  return { group, choices, suggestion: { name: best.name, reason } };
}

/** What the saved workflows say about one vault model, over all its names. */
export interface ModelUsage {
  /**
   * Null when there is no answer for any of its names, false when there were
   * no saved workflow files to search, so "not named" means nothing.
   */
  searched: boolean | null;
  /** Each workflow once, however many of the model's names it uses. */
  matches: readonly UsageMatch[];
  /** The engine's sentence for what the check did. */
  method: string | null;
}

export function usageOfModel(
  file: Pick<VaultFile, "canonicalName" | "aliases">,
  answers: ReadonlyMap<string, UsageResult>,
): ModelUsage {
  const found = [file.canonicalName, ...file.aliases]
    .map((name) => answers.get(name))
    .filter((a): a is UsageResult => a !== undefined);
  if (found.length === 0) return { searched: null, matches: [], method: null };
  const seen = new Set<string>();
  const matches: UsageMatch[] = [];
  for (const answer of found) {
    for (const match of answer.matches) {
      if (seen.has(match.workflowPath.toLowerCase())) continue;
      seen.add(match.workflowPath.toLowerCase());
      matches.push(match);
    }
  }
  const searched = found.some((a) => a.searched);
  return {
    searched,
    matches,
    method: (found.find((a) => a.searched === searched) ?? found[0]!).method,
  };
}

/** The sentence the engine requires next to every used or not-used answer. */
export function usageMethodOf(results: readonly UsageResult[]): string | null {
  return results[0]?.method ?? null;
}
