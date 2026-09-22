/**
 * The development engine.
 *
 * It implements the same Engine interface the Tauri client implements, so every
 * screen runs against it exactly as it runs against the real thing: the same
 * progress events, the same refusals, the same result shapes. It reads nothing
 * from disk. It never sees a real ComfyUI install.
 */

import { derivePlan } from "~/domain/plan";
import type { PlannedModel } from "~/domain/plan";
import { joinPath, leafOf } from "~/domain/format";
import type {
  ApplyProgress,
  ApplyResult,
  CreateFolderResult,
  Engine,
  FolderCheck,
  FolderEntry,
  InstanceId,
  MachineState,
  Model,
  MoveLogLine,
  PickerPurpose,
  Placement,
  ScanProgress,
  ScanResult,
  ScanStepId,
  SkippedFile,
  Unsubscribe,
} from "~/ipc/contract";
import {
  FIXTURE_COMFY_PROCESS,
  FIXTURE_MODELS,
  fixtureMachine,
  fixtureScan,
} from "~/ipc/fixture/dataset";

const MB = 1024 * 1024;

/** How long a fake scan and a fake run take, in milliseconds. */
const SCAN_MS = 18_000;
const APPLY_MS = 11_000;
const TICK_MS = 90;

/** Where each scan step ends, as a fraction of the whole scan. */
const SCAN_STEPS: ReadonlyArray<{ id: ScanStepId; end: number }> = [
  { id: "read_folders", end: 0.048 },
  { id: "read_yaml", end: 0.104 },
  { id: "list_files", end: 0.208 },
  { id: "hash_files", end: 0.872 },
  { id: "civitai", end: 1 },
];

// ── the fake disk the folder picker walks ───────────────────────────────────

interface FakeFolder {
  readonly path: string;
  readonly children: readonly string[];
  readonly readable?: boolean;
}

const MODEL_SUBFOLDERS = [
  "checkpoints",
  "clip_vision",
  "controlnet",
  "diffusion_models",
  "loras",
  "text_encoders",
  "upscale_models",
  "vae",
];

function installFolders(root: string): FakeFolder[] {
  return [
    { path: root, children: [`${root}\\models`, `${root}\\custom_nodes`, `${root}\\output`] },
    {
      path: `${root}\\models`,
      children: MODEL_SUBFOLDERS.map((f) => `${root}\\models\\${f}`),
    },
    ...MODEL_SUBFOLDERS.map((f) => ({
      path: `${root}\\models\\${f}`,
      children: [],
    })),
    { path: `${root}\\custom_nodes`, children: [] },
    { path: `${root}\\output`, children: [] },
  ];
}

const FAKE_DISK: FakeFolder[] = [
  {
    path: "C:\\",
    children: [
      "C:\\ComfyUI-Alpha",
      "C:\\ComfyUI-Beta",
      "C:\\ComfyUI-Portable",
      "C:\\ComfyVault",
      "C:\\Program Files",
      "C:\\Users",
    ],
  },
  ...installFolders("C:\\ComfyUI-Alpha"),
  ...installFolders("C:\\ComfyUI-Beta"),
  ...installFolders("C:\\ComfyUI-Portable"),
  {
    path: "C:\\ComfyVault",
    children: MODEL_SUBFOLDERS.map((f) => `C:\\ComfyVault\\${f}`),
  },
  ...MODEL_SUBFOLDERS.map((f) => ({ path: `C:\\ComfyVault\\${f}`, children: [] })),
  { path: "C:\\Program Files", children: ["C:\\Program Files\\NVIDIA Corporation"], readable: false },
  { path: "C:\\Program Files\\NVIDIA Corporation", children: [] },
  { path: "C:\\Users", children: ["C:\\Users\\alex"] },
  {
    path: "C:\\Users\\alex",
    children: ["C:\\Users\\alex\\Downloads", "C:\\Users\\alex\\Documents"],
  },
  { path: "C:\\Users\\alex\\Downloads", children: [] },
  { path: "C:\\Users\\alex\\Documents", children: [] },
  { path: "D:\\", children: ["D:\\ai-models", "D:\\ComfyUI-Backup"] },
  { path: "D:\\ai-models", children: ["D:\\ai-models\\ltx"] },
  { path: "D:\\ai-models\\ltx", children: [] },
  ...installFolders("D:\\ComfyUI-Backup"),
];

/** What a peek into a folder that is not registered turns up. */
const PEEK: Record<string, { folders: number; files: number; bytes: number; yaml: boolean }> = {
  "C:\\ComfyUI-Portable": { folders: 6, files: 31, bytes: 118000 * MB, yaml: false },
  "D:\\ComfyUI-Backup": { folders: 5, files: 22, bytes: 96000 * MB, yaml: false },
};
const PEEK_DEFAULT = { folders: 8, files: 147, bytes: 1044000 * MB, yaml: false };

// ── the engine ──────────────────────────────────────────────────────────────

/** Every copy a running ComfyUI was holding becomes movable again. */
function clearOpenBlocks(scan: ScanResult): ScanResult {
  return {
    ...scan,
    models: scan.models.map((m) => ({
      ...m,
      placements: m.placements.map((p) =>
        p.blocked?.kind === "file_open" ? { ...p, blocked: null } : p,
      ),
    })),
  };
}

type Listener<T> = (value: T) => void;

class Emitter<T> {
  private listeners = new Set<Listener<T>>();
  on(fn: Listener<T>): Unsubscribe {
    this.listeners.add(fn);
    return () => this.listeners.delete(fn);
  }
  emit(value: T): void {
    for (const fn of [...this.listeners]) fn(value);
  }
}

export class FixtureEngine implements Engine {
  private scan: ScanResult | null;
  private machine: MachineState;
  private disk: FakeFolder[] = FAKE_DISK.map((f) => ({ ...f, children: [...f.children] }));
  private run: ApplyResult | null = null;
  /** The scan as it was before the last run, so a revert has something to go back to. */
  private beforeRun: ScanResult | null = null;
  private freeBeforeRun = 0;

  private scanTimer: ReturnType<typeof setInterval> | null = null;
  private applyTimer: ReturnType<typeof setInterval> | null = null;
  private applyStopping = false;

  private scanProgress = new Emitter<ScanProgress>();
  private scanFinished = new Emitter<ScanResult>();
  private scanCancelled = new Emitter<void>();
  private applyProgress = new Emitter<ApplyProgress>();
  private applyFinished = new Emitter<ApplyResult>();

  constructor(options: { empty?: boolean } = {}) {
    this.scan = options.empty === true ? null : fixtureScan();
    this.machine = fixtureMachine();
    if (options.empty === true) {
      this.scan = null;
      this.machine = fixtureMachine({ running: [] });
    }
  }

  async loadScan(): Promise<ScanResult | null> {
    return this.scan;
  }

  async readMachine(): Promise<MachineState> {
    return this.machine;
  }

  // ── scan ──────────────────────────────────────────────────────────────────

  async startScan(): Promise<void> {
    this.stopScanTimer();
    const models = this.scan?.models ?? FIXTURE_MODELS;
    const filesToHash = models.reduce((s, m) => s + m.placements.length, 0);
    const bytesToHash = models.reduce(
      (s, m) => s + m.bytes * Math.max(m.placements.length, 1),
      0,
    );
    const started = Date.now();

    this.scanTimer = setInterval(() => {
      const elapsed = Date.now() - started;
      const overall = Math.min(1, elapsed / SCAN_MS);
      this.scanProgress.emit(
        this.buildScanProgress(overall, models, filesToHash, bytesToHash),
      );
      if (overall >= 1) {
        this.stopScanTimer();
        const result = fixtureScan(models);
        this.scan = result;
        this.scanFinished.emit(result);
      }
    }, TICK_MS);
  }

  private buildScanProgress(
    overall: number,
    models: readonly Model[],
    filesToHash: number,
    bytesToHash: number,
  ): ScanProgress {
    const step = SCAN_STEPS.find((s) => overall < s.end) ?? SCAN_STEPS[SCAN_STEPS.length - 1]!;
    const hashStart = SCAN_STEPS[2]!.end;
    const hashEnd = SCAN_STEPS[3]!.end;
    const hashed = Math.max(
      0,
      Math.min(1, (overall - hashStart) / (hashEnd - hashStart)),
    );
    const done = (id: ScanStepId) =>
      overall >= (SCAN_STEPS.find((s) => s.id === id)?.end ?? 1);

    const index = Math.min(
      models.length - 1,
      Math.floor(hashed * models.length),
    );
    const model = models[index];
    const placement = model?.placements[0];

    const duplicateCopies = models.reduce(
      (s, m) => s + m.placements.filter((p) => p.blocked === null).length - (m.placements.some((p) => p.blocked === null) ? 1 : 0),
      0,
    );
    const reclaim = models.reduce((s, m) => {
      const live = m.placements.filter((p) => p.blocked === null).length;
      return s + Math.max(0, live - 1) * m.bytes;
    }, 0);

    return {
      currentStep: step.id,
      overall,
      etaSeconds: Math.max(0, Math.round((SCAN_MS * (1 - overall)) / 1000)),
      instancesRead: done("read_folders") ? this.instanceCount() : null,
      extraFolders: done("read_yaml")
        ? (this.scan?.instances.flatMap((i) => i.extraModelPaths ?? []) ?? [])
        : null,
      filesListed: done("list_files") ? filesToHash : null,
      bytesListed: done("list_files") ? bytesToHash : null,
      filesHashed: Math.round(filesToHash * hashed),
      filesToHash,
      bytesHashed: Math.round(bytesToHash * hashed),
      bytesToHash,
      civitaiMatched: done("civitai")
        ? models.filter((m) => m.civitai !== null).length
        : null,
      current:
        model && hashed > 0 && hashed < 1
          ? { name: model.filename, path: placement?.fullPath ?? model.filename }
          : null,
      found:
        hashed > 0
          ? {
              models: Math.round(models.length * hashed),
              duplicateCopies: Math.round(duplicateCopies * hashed),
              reclaimableBytes: Math.round(reclaim * hashed),
            }
          : null,
    };
  }

  private instanceCount(): number {
    return this.scan?.instances.length ?? 0;
  }

  async cancelScan(): Promise<void> {
    if (!this.scanTimer) return;
    this.stopScanTimer();
    this.scanCancelled.emit();
  }

  private stopScanTimer(): void {
    if (this.scanTimer) clearInterval(this.scanTimer);
    this.scanTimer = null;
  }

  onScanProgress(fn: (p: ScanProgress) => void): Unsubscribe {
    return this.scanProgress.on(fn);
  }
  onScanFinished(fn: (r: ScanResult) => void): Unsubscribe {
    return this.scanFinished.on(fn);
  }
  onScanCancelled(fn: () => void): Unsubscribe {
    return this.scanCancelled.on(() => fn());
  }

  // ── apply ─────────────────────────────────────────────────────────────────

  async startApply(modelIds: string[]): Promise<void> {
    const scan = this.scan;
    if (!scan) return;
    this.stopApplyTimer();
    this.applyStopping = false;
    this.beforeRun = structuredClone(scan);
    this.freeBeforeRun = this.machine.vaultDrive.freeBytes;

    const plan = derivePlan(scan, this.machine);
    const picked = modelIds
      .map((id) => plan.byId.get(id))
      .filter((m): m is NonNullable<typeof m> => m != null && m.live.length > 0);

    const filesTotal = picked.length;
    const linksTotal = picked.reduce((s, m) => s + m.live.length, 0);
    const bytesTotal = picked.reduce((s, m) => s + m.reclaimBytes, 0);
    // The engine stops on a file that changed since the report was made.
    const skipAt = picked.length > 7 ? 6 : -1;

    const started = Date.now();
    const log: MoveLogLine[] = [];
    const skipped: SkippedFile[] = [];
    let emitted = 0;

    this.applyTimer = setInterval(() => {
      const elapsed = Date.now() - started;
      const target = this.applyStopping
        ? Math.min(1, (elapsed + APPLY_MS * 0.06) / APPLY_MS)
        : Math.min(1, elapsed / APPLY_MS);
      const overall = this.applyStopping
        ? Math.min(1, elapsed / (APPLY_MS * 0.5))
        : target;
      const upto = Math.min(picked.length, Math.floor(overall * picked.length));

      while (emitted < upto) {
        const model = picked[emitted]!;
        const at = new Date(started + emitted * 900).toISOString();
        if (emitted === skipAt) {
          skipped.push({
            modelId: model.id,
            filename: model.filename,
            bytes: model.bytes,
            reason: { kind: "changed_since_report" },
          });
          log.push({
            at,
            verb: "skip",
            detail: `${model.filename}  \u00b7  changed since the report`,
          });
        } else {
          const keeper = model.keeper!;
          log.push({
            at,
            verb: "move",
            detail: `${this.instanceName(keeper.instanceId)}\\${keeper.folder}${keeper.filename}  \u2192  vault\\${model.folder}\\${model.vaultName}`,
          });
          for (const p of model.live) {
            log.push({
              at,
              verb: "link",
              detail: `${this.instanceName(p.instanceId)}\\${p.folder}${p.filename}`,
            });
          }
          for (const p of model.blocked) {
            log.push({
              at,
              verb: "skip",
              detail: `${this.instanceName(p.instanceId)}\\${p.folder}${p.filename}  \u00b7  ${p.blocked?.kind ?? "blocked"}`,
            });
          }
        }
        emitted += 1;
      }

      const movedModels = picked.slice(0, emitted).filter((_, i) => i !== skipAt);
      const progress: ApplyProgress = {
        overall,
        etaSeconds: Math.max(0, Math.round((APPLY_MS * (1 - overall)) / 1000)),
        filesMoved: movedModels.length,
        filesTotal,
        linksCreated: movedModels.reduce((s, m) => s + m.live.length, 0),
        linksTotal,
        bytesMoved: movedModels.reduce((s, m) => s + m.reclaimBytes, 0),
        bytesTotal,
        current: this.currentMove(picked, overall),
        skipped: [...skipped],
        log: [...log],
        stopping: this.applyStopping,
      };
      this.applyProgress.emit(progress);

      if (overall >= 1) {
        this.stopApplyTimer();
        this.finishApply(picked, log, skipped, progress);
      }
    }, TICK_MS);
  }

  private currentMove(
    picked: readonly PlannedModel[],
    overall: number,
  ): ApplyProgress["current"] {
    if (picked.length === 0) return null;
    const model = picked[Math.min(picked.length - 1, Math.floor(overall * picked.length))]!;
    const keeper = model.keeper;
    const link = model.liveExtras[0] ?? keeper;
    if (!keeper || !link) return null;
    return {
      name: model.filename,
      fromInstance: this.instanceName(keeper.instanceId),
      fromPath: keeper.folder,
      toPath: model.vaultPath,
      linkInstance: this.instanceName(link.instanceId),
      linkPath: link.folder,
    };
  }

  private finishApply(
    picked: readonly PlannedModel[],
    log: MoveLogLine[],
    skipped: SkippedFile[],
    progress: ApplyProgress,
  ): void {
    const scan = this.scan;
    if (!scan) return;
    const skippedIds = new Set(skipped.map((s) => s.modelId));
    const movedIds = new Set(
      picked.filter((m) => !skippedIds.has(m.id)).map((m) => m.id),
    );

    // The files are in the vault now and every place they were holds a link.
    const now = new Date().toISOString();
    const models = scan.models.map((m) => {
      if (!movedIds.has(m.id)) return m;
      const placements: Placement[] = m.placements.map((p) =>
        p.blocked === null ? { ...p, isLink: true } : p,
      );
      return { ...m, placements, inVaultSince: now };
    });
    this.scan = { ...scan, models };
    this.machine = {
      ...this.machine,
      vaultDrive: {
        ...this.machine.vaultDrive,
        freeBytes: this.machine.vaultDrive.freeBytes + progress.bytesMoved,
      },
    };

    const plan = derivePlan(this.scan, this.machine);
    const result: ApplyResult = {
      runId: `run-${Date.now()}`,
      startedAt: log[0]?.at ?? now,
      finishedAt: now,
      filesMoved: progress.filesMoved,
      linksCreated: progress.linksCreated,
      bytesFreed: progress.bytesMoved,
      renamedInVault: plan.clashes.reduce((s, g) => s + g.models.length - 1, 0),
      leftAlone: {
        files: plan.blocked.length,
        bytes: plan.totals.blockedBytes,
      },
      skipped,
      log,
      logPath: `${this.machine.vaultPath}\\moves.log`,
      revertable: true,
    };
    this.run = result;
    this.applyFinished.emit(result);
  }

  async stopApply(): Promise<void> {
    this.applyStopping = true;
  }

  private stopApplyTimer(): void {
    if (this.applyTimer) clearInterval(this.applyTimer);
    this.applyTimer = null;
  }

  onApplyProgress(fn: (p: ApplyProgress) => void): Unsubscribe {
    return this.applyProgress.on(fn);
  }
  onApplyFinished(fn: (r: ApplyResult) => void): Unsubscribe {
    return this.applyFinished.on(fn);
  }

  async lastRun(): Promise<ApplyResult | null> {
    return this.run;
  }

  async revert(runId: string): Promise<void> {
    if (!this.run || this.run.runId !== runId || !this.beforeRun) return;
    this.scan = this.beforeRun;
    this.machine = {
      ...this.machine,
      vaultDrive: { ...this.machine.vaultDrive, freeBytes: this.freeBeforeRun },
    };
    this.beforeRun = null;
    this.run = null;
  }

  // ── folder picker ─────────────────────────────────────────────────────────

  async listFolder(
    path: string | null,
    purpose: PickerPurpose,
  ): Promise<FolderEntry[]> {
    if (path === null) {
      if (purpose === "link") {
        // A link belongs inside an install, so the picker starts at the model
        // folders of the installs that are registered.
        return (this.scan?.instances ?? []).map((i) => ({
          path: `${i.path}\\models`,
          name: `${i.name}  \\models`,
          kind: "folder" as const,
          looksLikeInstall: null,
          readable: true,
        }));
      }
      return this.disk
        .filter((f) => /^[A-Za-z]:\\$/.test(f.path))
        .map((f) => ({
          path: f.path,
          name: f.path,
          kind: "drive" as const,
          looksLikeInstall: null,
          readable: true,
        }));
    }
    const folder = this.disk.find((f) => f.path === path);
    if (!folder || folder.readable === false) return [];
    return folder.children.map((child) => ({
      path: child,
      name: leafOf(child),
      kind: "folder" as const,
      looksLikeInstall:
        purpose === "instance" ? this.hasModelsFolder(child) : null,
      readable: this.disk.find((f) => f.path === child)?.readable !== false,
    }));
  }

  private hasModelsFolder(path: string): boolean {
    const folder = this.disk.find((f) => f.path === path);
    return folder?.children.some((c) => leafOf(c).toLowerCase() === "models") ?? false;
  }

  async checkFolder(path: string, purpose: PickerPurpose): Promise<FolderCheck> {
    if (purpose === "instance") {
      const existing = this.scan?.instances.find(
        (i) => i.path.toLowerCase() === path.toLowerCase(),
      );
      if (existing) {
        return { for: "instance", ok: false, reason: "already_registered", instanceId: existing.id };
      }
      const folder = this.disk.find((f) => f.path === path);
      if (folder?.readable === false) {
        return { for: "instance", ok: false, reason: "unreadable" };
      }
      if (!this.hasModelsFolder(path)) {
        return { for: "instance", ok: false, reason: "no_models_folder" };
      }
      const peek = PEEK[path] ?? PEEK_DEFAULT;
      return {
        for: "instance",
        ok: true,
        modelFolders: peek.folders,
        files: peek.files,
        bytes: peek.bytes,
        hasExtraModelPaths: peek.yaml,
        onDifferentDrive:
          path.slice(0, 2).toUpperCase() !==
          this.machine.vaultPath.slice(0, 2).toUpperCase(),
        wasRemoved: this.scan?.removedInstance?.path === path,
      };
    }

    if (purpose === "vault") {
      const inside = this.scan?.instances.find((i) =>
        path.toLowerCase().startsWith(i.path.toLowerCase() + "\\"),
      );
      if (inside) {
        return { for: "vault", ok: false, reason: "inside_an_install", instanceId: inside.id };
      }
      const folder = this.disk.find((f) => f.path === path);
      if (folder?.readable === false) {
        return { for: "vault", ok: false, reason: "not_writable" };
      }
      const drive = path.slice(0, 2).toUpperCase();
      const others = (this.scan?.instances ?? []).filter(
        (i) => i.path.slice(0, 2).toUpperCase() !== drive,
      );
      return {
        for: "vault",
        ok: true,
        drive,
        freeBytes:
          drive === this.machine.vaultDrive.letter
            ? this.machine.vaultDrive.freeBytes
            : 421_000 * MB,
        sameDriveAsInstalls: others.length === 0,
        installsOnOtherDrives: others.map((i) => i.name),
      };
    }

    const owner = this.scan?.instances.find((i) =>
      path.toLowerCase().startsWith(i.path.toLowerCase() + "\\"),
    );
    if (!owner) return { for: "link", ok: false, reason: "outside_every_install" };
    const folder = this.disk.find((f) => f.path === path);
    if (folder?.readable === false) {
      return { for: "link", ok: false, reason: "not_writable" };
    }
    return { for: "link", ok: true, instanceId: owner.id };
  }

  async createFolder(parent: string, name: string): Promise<CreateFolderResult> {
    const parentFolder = this.disk.find((f) => f.path === parent);
    if (!parentFolder) return { ok: false, reason: "invalid_name" };
    if (parentFolder.readable === false) return { ok: false, reason: "denied" };
    const path = joinPath(parent, name);
    if (this.disk.some((f) => f.path.toLowerCase() === path.toLowerCase())) {
      return { ok: false, reason: "exists" };
    }
    (parentFolder.children as string[]).push(path);
    this.disk.push({ path, children: [] });
    return { ok: true, path };
  }

  // ── changes the person makes ───────────────────────────────────────────────

  async addInstance(path: string): Promise<ScanResult> {
    const scan = this.scan ?? fixtureScan([]);
    const id = leafOf(path).toLowerCase().replace(/[^a-z0-9]+/g, "-");
    const instance = {
      id,
      name: leafOf(path).replace(/^ComfyUI-?/i, "") || leafOf(path),
      path,
      running: false,
      extraModelPaths: null,
      addedAt: new Date().toISOString(),
    };
    this.scan = { ...scan, instances: [...scan.instances, instance] };
    return this.scan;
  }

  async setInstancePath(id: InstanceId, path: string): Promise<ScanResult> {
    const scan = this.scan;
    if (!scan) throw new Error("nothing scanned");
    this.scan = {
      ...scan,
      instances: scan.instances.map((i) => (i.id === id ? { ...i, path } : i)),
    };
    return this.scan;
  }

  async removeInstance(id: InstanceId): Promise<ScanResult> {
    const scan = this.scan;
    if (!scan) throw new Error("nothing scanned");
    this.scan = {
      ...scan,
      instances: scan.instances.filter((i) => i.id !== id),
      models: scan.models.map((m) => ({
        ...m,
        placements: m.placements.filter((p) => p.instanceId !== id),
      })),
    };
    return this.scan;
  }

  async setVaultPath(path: string): Promise<MachineState> {
    this.machine = { ...this.machine, vaultPath: path };
    return this.machine;
  }

  async setCivitaiEnabled(on: boolean): Promise<void> {
    if (this.scan) this.scan = { ...this.scan, civitaiEnabled: on };
  }

  async setVaultName(modelId: string, name: string): Promise<ScanResult> {
    const scan = this.scan;
    if (!scan) throw new Error("nothing scanned");
    this.scan = {
      ...scan,
      models: scan.models.map((m) =>
        m.id === modelId ? { ...m, filename: name } : m,
      ),
    };
    return this.scan;
  }

  async dropName(modelId: string, name: string): Promise<ScanResult> {
    const scan = this.scan;
    if (!scan) throw new Error("nothing scanned");
    this.scan = {
      ...scan,
      models: scan.models.map((m) =>
        m.id === modelId
          ? { ...m, placements: m.placements.filter((p) => p.filename !== name) }
          : m,
      ),
    };
    return this.scan;
  }

  async addLink(modelId: string, folder: string): Promise<ScanResult> {
    const scan = this.scan;
    if (!scan) throw new Error("nothing scanned");
    const owner = scan.instances.find((i) =>
      folder.toLowerCase().startsWith(i.path.toLowerCase() + "\\"),
    );
    this.scan = {
      ...scan,
      models: scan.models.map((m) => {
        if (m.id !== modelId || !owner) return m;
        const relative = `${folder.slice(owner.path.length + 1)}\\`;
        return {
          ...m,
          placements: [
            ...m.placements,
            {
              id: `${m.id}-link-${m.placements.length}`,
              instanceId: owner.id,
              folder: relative,
              filename: m.filename,
              fullPath: `${folder}\\${m.filename}`,
              isLink: true,
              blocked: null,
            },
          ],
        };
      }),
    };
    return this.scan;
  }

  async deleteOrphan(modelId: string): Promise<ScanResult> {
    const scan = this.scan;
    if (!scan) throw new Error("nothing scanned");
    const model = scan.models.find((m) => m.id === modelId);
    this.scan = { ...scan, models: scan.models.filter((m) => m.id !== modelId) };
    if (model) {
      this.machine = {
        ...this.machine,
        vaultDrive: {
          ...this.machine.vaultDrive,
          freeBytes: this.machine.vaultDrive.freeBytes + model.bytes,
        },
      };
    }
    return this.scan;
  }

  async markCopyInstead(modelId: string, placementId: string): Promise<ScanResult> {
    const scan = this.scan;
    if (!scan) throw new Error("nothing scanned");
    this.scan = {
      ...scan,
      models: scan.models.map((m) =>
        m.id === modelId
          ? {
              ...m,
              placements: m.placements.map((p) =>
                p.id === placementId && p.blocked?.kind === "other_drive"
                  ? { ...p, blocked: null }
                  : p,
              ),
            }
          : m,
      ),
    };
    return this.scan;
  }

  async openWindowsDeveloperSettings(): Promise<void> {
    // The real engine opens ms-settings:developers. There is no Windows here.
  }

  async openInExplorer(): Promise<void> {
    // The real engine opens Explorer. There is no Explorer here.
  }

  async windowMinimize(): Promise<void> {}
  async windowToggleMaximize(): Promise<void> {}
  async windowClose(): Promise<void> {}

  // ── development affordances, used by the dev panel only ───────────────────

  /** Flip Developer Mode, the way turning it on in Windows would. */
  devSetDeveloperMode(on: boolean): void {
    this.machine = { ...this.machine, developerMode: on };
  }

  /** Close or reopen ComfyUI, the way the person would. */
  devSetComfyRunning(running: boolean): void {
    this.machine = {
      ...this.machine,
      running: running ? [FIXTURE_COMFY_PROCESS] : [],
    };
    const scan = this.scan;
    if (!scan) return;
    const withInstances: ScanResult = {
      ...scan,
      instances: scan.instances.map((i) =>
        i.id === FIXTURE_COMFY_PROCESS.instanceId ? { ...i, running } : i,
      ),
    };
    this.scan = running
      ? this.restoreOpenBlocks(withInstances)
      : clearOpenBlocks(withInstances);
  }

  private restoreOpenBlocks(scan: ScanResult): ScanResult {
    const original = fixtureScan();
    const openPaths = new Set(
      original.models.flatMap((m) =>
        m.placements
          .filter((p) => p.blocked?.kind === "file_open")
          .map((p) => p.fullPath),
      ),
    );
    return {
      ...scan,
      models: scan.models.map((m) => ({
        ...m,
        placements: m.placements.map((p) =>
          openPaths.has(p.fullPath) && p.blocked === null
            ? {
                ...p,
                blocked: {
                  kind: "file_open" as const,
                  process: FIXTURE_COMFY_PROCESS.process,
                  pid: FIXTURE_COMFY_PROCESS.pid,
                  instanceId: FIXTURE_COMFY_PROCESS.instanceId,
                },
              }
            : p,
        ),
      })),
    };
  }

  /** Throw the app back to the state a first run is in. */
  devReset(empty: boolean): void {
    this.stopScanTimer();
    this.stopApplyTimer();
    this.scan = empty ? null : fixtureScan();
    this.machine = fixtureMachine(empty ? { running: [] } : {});
    this.run = null;
    this.beforeRun = null;
    this.disk = FAKE_DISK.map((f) => ({ ...f, children: [...f.children] }));
  }

  private instanceName(id: InstanceId): string {
    return this.scan?.instances.find((i) => i.id === id)?.name ?? id;
  }
}
