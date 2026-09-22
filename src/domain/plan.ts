/**
 * Turn a scan result plus the state of the machine into the plan the
 * Consolidate screen shows.
 *
 * This is a pure function. It never touches the engine and never touches the
 * store, so every rule in it is testable on its own: which copy is kept, which
 * copies become links, what a filename clash is renamed to, what each install
 * gives back, and what cannot move.
 *
 * Two rules here decide what the engine later writes to disk. comfyvault-core
 * must use the same two, or the report lies:
 *
 *   1. THE KEPT COPY is the first copy that can move, in the order the person
 *      registered the installs.
 *   2. A FILENAME CLASH is settled by size: the largest file keeps the plain
 *      name, the next gets "-2" before the extension, then "-3". Files of equal
 *      size are ordered by hash, so the answer is the same on every run.
 */

import type {
  BlockedKind,
  BlockedReason,
  MachineState,
  Model,
  ModelFolder,
  Placement,
  ScanResult,
} from "~/ipc/contract";

export interface PlannedModel {
  readonly model: Model;
  readonly id: string;
  readonly filename: string;
  readonly folder: ModelFolder;
  readonly bytes: number;
  /** The name this file carries inside the vault. */
  readonly vaultName: string;
  readonly vaultPath: string;
  /** The copy that moves into the vault. Null when nothing can move. */
  readonly keeper: Placement | null;
  /** Every copy that is not the kept one. */
  readonly extras: readonly Placement[];
  /** Extras that can move, so they become links. */
  readonly liveExtras: readonly Placement[];
  /** Copies that cannot move right now, with the reason. */
  readonly blocked: readonly Placement[];
  /** Copies that can move, kept one included. Each gets a link. */
  readonly live: readonly Placement[];
  /**
   * Extras that are still real files. A path that already holds a link takes no
   * room, so turning it into a link again gives nothing back.
   */
  readonly reclaimableExtras: readonly Placement[];
  /** What this model gives back: one file's size per copy that becomes a link. */
  readonly reclaimBytes: number;
  /** There is still a real file here to move or to replace with a link. */
  readonly needsWork: boolean;
  /** Names other than the filename that these bytes are stored under. */
  readonly altNames: readonly string[];
  /** Every name these bytes answer to, the filename first. */
  readonly allNames: readonly string[];
  /** Nothing on disk points at it. It sits in the vault alone. */
  readonly isOrphan: boolean;
}

export interface ClashGroup {
  readonly filename: string;
  readonly models: readonly PlannedModel[];
}

export interface BlockedPlacement {
  readonly model: PlannedModel;
  readonly placement: Placement;
  readonly reason: BlockedReason;
}

export interface InstanceTotals {
  readonly bytes: number;
  readonly files: number;
  /** Files that become links. */
  readonly moving: number;
  readonly movingBytes: number;
  /** Files that stay put. */
  readonly stuck: number;
  readonly stuckBytes: number;
}

export interface PlanTotals {
  /** One copy of every model. */
  readonly uniqueBytes: number;
  /** What the models take on disk today, every copy counted. */
  readonly onDiskBytes: number;
  /** What the whole plan gives back. */
  readonly reclaimBytes: number;
  /** Model files on disk today, every copy counted. */
  readonly files: number;
  /** Copies held more than once. These are the ones that become links. */
  readonly duplicateCopies: number;
  /** What cannot move right now. */
  readonly blockedBytes: number;
  /** What the vault already holds with nothing pointing at it. */
  readonly vaultOnlyBytes: number;
  /** Models whose name appears in no workflow file. */
  readonly unused: number;
  readonly countedNeverMovedBytes: number;
  readonly perInstance: ReadonlyMap<string, InstanceTotals>;
}

export interface Plan {
  readonly models: readonly PlannedModel[];
  readonly byId: ReadonlyMap<string, PlannedModel>;
  /** Held twice or more, biggest win first. This is the easy win. */
  readonly duplicates: readonly PlannedModel[];
  /** Same filename, different bytes. */
  readonly clashes: readonly ClashGroup[];
  /** Exists once. Moves into the vault and frees nothing. */
  readonly singles: readonly PlannedModel[];
  readonly blocked: readonly BlockedPlacement[];
  readonly orphans: readonly PlannedModel[];
  /** Same bytes under more than one name. Cleanup settles these. */
  readonly aliases: readonly PlannedModel[];
  readonly totals: PlanTotals;
  /** Every folder kind present, for the Library filter. */
  readonly folders: readonly string[];
}

export interface DeriveOptions {
  /**
   * Treat files a running ComfyUI holds open as movable. The Consolidate screen
   * uses this to say what closing ComfyUI is worth, without changing the plan.
   */
  readonly ignoreOpenFiles?: boolean;
}

/** Blocked copies are listed grouped by reason, in this order. */
const BLOCKED_ORDER: Record<BlockedKind, number> = {
  other_drive: 0,
  file_open: 1,
  permission_denied: 2,
};

function vaultPathFor(vaultRoot: string, folder: string, name: string): string {
  const root = vaultRoot.endsWith("\\") ? vaultRoot.slice(0, -1) : vaultRoot;
  return `${root}\\${folder}\\${name}`;
}

/** "flux1-dev.safetensors" + 2 -> "flux1-dev-2.safetensors" */
function suffixed(filename: string, n: number): string {
  const dot = filename.lastIndexOf(".");
  if (dot <= 0) return `${filename}-${n}`;
  return `${filename.slice(0, dot)}-${n}${filename.slice(dot)}`;
}

/**
 * Order the copies of one model the way the person registered the installs, so
 * "the first install you registered" is true by construction. Copies inside one
 * install keep the order the engine reported them in.
 */
function orderPlacements(
  placements: readonly Placement[],
  instanceRank: ReadonlyMap<string, number>,
): Placement[] {
  return placements
    .map((p, i) => ({ p, i }))
    .sort((a, b) => {
      const ra = instanceRank.get(a.p.instanceId) ?? Number.MAX_SAFE_INTEGER;
      const rb = instanceRank.get(b.p.instanceId) ?? Number.MAX_SAFE_INTEGER;
      return ra - rb || a.i - b.i;
    })
    .map((x) => x.p);
}

/** Settle the vault name of every model, including filename clashes. */
function assignVaultNames(models: readonly Model[]): Map<string, string> {
  const byFilename = new Map<string, Model[]>();
  for (const m of models) {
    const list = byFilename.get(m.filename);
    if (list) list.push(m);
    else byFilename.set(m.filename, [m]);
  }
  const names = new Map<string, string>();
  for (const [filename, group] of byFilename) {
    if (group.length === 1) {
      names.set(group[0]!.id, filename);
      continue;
    }
    const ordered = [...group].sort(
      (a, b) => b.bytes - a.bytes || a.sha256.localeCompare(b.sha256),
    );
    ordered.forEach((m, i) => {
      names.set(m.id, i === 0 ? filename : suffixed(filename, i + 1));
    });
  }
  return names;
}

export function derivePlan(
  scan: ScanResult,
  machine: MachineState,
  options: DeriveOptions = {},
): Plan {
  const ignoreOpen = options.ignoreOpenFiles === true;
  const instanceRank = new Map(scan.instances.map((inst, i) => [inst.id, i]));
  const vaultNames = assignVaultNames(scan.models);

  const planned: PlannedModel[] = scan.models.map((model) => {
    const placements = orderPlacements(model.placements, instanceRank);
    const blockOf = (p: Placement): BlockedReason | null => {
      if (!p.blocked) return null;
      if (ignoreOpen && p.blocked.kind === "file_open") return null;
      return p.blocked;
    };

    const live = placements.filter((p) => blockOf(p) === null);
    const blocked = placements.filter((p) => blockOf(p) !== null);
    // A path that already holds a link is not a candidate to keep: the real file
    // is in the vault, and the kept copy has to be a real file.
    const keeper = live.find((p) => !p.isLink) ?? live[0] ?? null;
    const extras = placements.filter((p) => p !== keeper);
    const liveExtras = live.filter((p) => p !== keeper);
    const reclaimableExtras = liveExtras.filter((p) => !p.isLink);

    const altNames = [
      ...new Set(
        placements.map((p) => p.filename).filter((n) => n !== model.filename),
      ),
    ];
    const vaultName = vaultNames.get(model.id) ?? model.filename;

    return {
      model,
      id: model.id,
      filename: model.filename,
      folder: model.folder,
      bytes: model.bytes,
      vaultName,
      vaultPath: vaultPathFor(machine.vaultPath, model.folder, vaultName),
      keeper,
      extras,
      liveExtras,
      blocked,
      live,
      reclaimableExtras,
      reclaimBytes: reclaimableExtras.length * model.bytes,
      needsWork: live.some((p) => !p.isLink),
      altNames,
      allNames: [model.filename, ...altNames],
      isOrphan: placements.length === 0,
    };
  });

  const byId = new Map(planned.map((p) => [p.id, p]));

  const clashCount = new Map<string, number>();
  for (const p of planned) {
    clashCount.set(p.filename, (clashCount.get(p.filename) ?? 0) + 1);
  }

  const duplicates = planned
    .filter((p) => p.reclaimableExtras.length > 0)
    .sort((a, b) => b.reclaimBytes - a.reclaimBytes);

  const clashes: ClashGroup[] = [...clashCount.entries()]
    .filter(([, n]) => n > 1)
    .map(([filename]) => ({
      filename,
      models: planned
        .filter((p) => p.filename === filename)
        .sort((a, b) => b.bytes - a.bytes || a.model.sha256.localeCompare(b.model.sha256)),
    }));

  const singles = planned
    .filter(
      (p) =>
        !p.isOrphan &&
        p.needsWork &&
        p.reclaimableExtras.length === 0 &&
        (clashCount.get(p.filename) ?? 0) === 1,
    )
    .sort((a, b) => b.bytes - a.bytes);

  const blocked: BlockedPlacement[] = [];
  for (const p of planned) {
    for (const placement of p.blocked) {
      const reason = placement.blocked;
      if (!reason) continue;
      blocked.push({ model: p, placement, reason });
    }
  }
  blocked.sort((a, b) => {
    const ra = BLOCKED_ORDER[a.reason.kind];
    const rb = BLOCKED_ORDER[b.reason.kind];
    return ra - rb || b.model.bytes - a.model.bytes;
  });

  const orphans = planned.filter((p) => p.isOrphan);
  const aliases = planned.filter((p) => p.altNames.length > 0);

  const totals = buildTotals(planned, scan, ignoreOpen);
  const folders = [...new Set(planned.map((p) => p.folder))].sort();

  return {
    models: planned,
    byId,
    duplicates,
    clashes,
    singles,
    blocked,
    orphans,
    aliases,
    totals,
    folders,
  };
}

function buildTotals(
  planned: readonly PlannedModel[],
  scan: ScanResult,
  ignoreOpen: boolean,
): PlanTotals {
  let uniqueBytes = 0;
  let onDiskBytes = 0;
  let reclaimBytes = 0;
  let files = 0;
  let duplicateCopies = 0;
  let blockedBytes = 0;
  let vaultOnlyBytes = 0;
  let unused = 0;

  const perInstance = new Map<string, InstanceTotals>();
  const bump = (id: string, patch: Partial<InstanceTotals>) => {
    const cur = perInstance.get(id) ?? {
      bytes: 0,
      files: 0,
      moving: 0,
      movingBytes: 0,
      stuck: 0,
      stuckBytes: 0,
    };
    perInstance.set(id, {
      bytes: cur.bytes + (patch.bytes ?? 0),
      files: cur.files + (patch.files ?? 0),
      moving: cur.moving + (patch.moving ?? 0),
      movingBytes: cur.movingBytes + (patch.movingBytes ?? 0),
      stuck: cur.stuck + (patch.stuck ?? 0),
      stuckBytes: cur.stuckBytes + (patch.stuckBytes ?? 0),
    });
  };
  for (const inst of scan.instances) bump(inst.id, {});

  for (const p of planned) {
    uniqueBytes += p.bytes;
    // Links take no room. What the models occupy is the real files on disk plus
    // the vault's own copy, once it holds one.
    const realFiles = p.model.placements.filter((x) => !x.isLink).length;
    const inVault = p.model.inVaultSince !== null ? 1 : 0;
    onDiskBytes += p.bytes * Math.max(realFiles + inVault, 1);
    reclaimBytes += p.reclaimBytes;
    files += p.model.placements.length;
    duplicateCopies += p.reclaimableExtras.length;
    blockedBytes += p.blocked.length * p.bytes;
    if (p.isOrphan) vaultOnlyBytes += p.bytes;
    if (p.model.workflowHits === 0) unused += 1;

    for (const placement of p.model.placements) {
      const isBlocked =
        placement.blocked !== null &&
        !(ignoreOpen && placement.blocked.kind === "file_open");
      bump(placement.instanceId, {
        bytes: p.bytes,
        files: 1,
        ...(isBlocked
          ? { stuck: 1, stuckBytes: p.bytes }
          : { moving: 1, movingBytes: p.bytes }),
      });
    }
  }

  const countedNeverMovedBytes = scan.countedNeverMoved.reduce(
    (sum, x) => sum + x.bytes,
    0,
  );

  return {
    uniqueBytes,
    onDiskBytes,
    reclaimBytes,
    files,
    duplicateCopies,
    blockedBytes,
    vaultOnlyBytes,
    unused,
    countedNeverMovedBytes,
    perInstance,
  };
}

/**
 * What the run gives back if every running ComfyUI is closed first. The
 * Consolidate screen prints the difference, so the person can decide whether
 * closing it is worth it.
 */
export function reclaimIfComfyClosed(
  scan: ScanResult,
  machine: MachineState,
): number {
  return derivePlan(scan, machine, { ignoreOpenFiles: true }).totals
    .reclaimBytes;
}
