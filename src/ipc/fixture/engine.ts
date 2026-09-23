/**
 * The development engine.
 *
 * It implements the same port the Tauri client implements and follows the same
 * rules docs/IPC-CONTRACT.md sets out, so a screen cannot pass against it and
 * fail against the real engine. It reads nothing from disk and never sees a
 * real ComfyUI install.
 */

import { fileNameOf } from "~/domain/view";
import { leafOf } from "~/domain/format";
import type {
  ApplyProgress,
  ApplyRecord,
  AppState,
  ConsolidationPlan,
  ContentFilter,
  ContentPage,
  DirectoryListing,
  DriveInfo,
  Engine,
  ExtraPath,
  Install,
  InstallCandidate,
  InterruptedApply,
  LinkRecord,
  LinkState,
  LinkWithState,
  LockState,
  ModelDirNode,
  ModelMetadata,
  NameGroup,
  PlanGroup,
  PlatformReport,
  RunningComfy,
  ScanEntryPage,
  ScanProgress,
  ScanRecord,
  Settings,
  Unsubscribe,
  UsageResult,
  VaultError,
  VaultFile,
  VaultFilePage,
  VaultHealth,
  VaultInfo,
} from "~/ipc/contract";
import {
  COUNTED_NEVER_MOVED,
  VAULT_ROOT,
  VAULT_TOTAL_BYTES,
  VAULT_VOLUME,
  buildWorld,
  contentRowsOf,
  nameGroupsOf,
  planOf,
  scanEntriesOf,
  scanResultOf,
  vaultFilesOf,
  type World,
} from "~/ipc/fixture/world";

const MB = 1024 * 1024;
/** How long a fake scan and a fake run take. Tests shorten them. */
/** What something else on the computer wrote during a run. */
const OTHER_ACTIVITY_BYTES = 734_003_200;

const SCAN_MS = 16_000;
const APPLY_MS = 10_000;
const TICK_MS = 100;

const SYMLINK_GUIDANCE =
  "Windows needs Developer Mode to create the links this app uses. Open " +
  "Settings, go to System, then For developers, and turn Developer Mode on. " +
  "You do not need to restart.";

const USAGE_METHOD =
  "The file name was searched for as plain text inside saved workflow files.";

const NOTHING_SEARCHED =
  "No saved workflow files were found, so nothing was searched. A workflow " +
  "that was never saved lives in the browser, where this app cannot see it.";

// ── the fake disk the folder picker walks ───────────────────────────────────

const MODEL_DIRS = [
  "checkpoints",
  "clip_vision",
  "controlnet",
  "diffusion_models",
  "loras",
  "text_encoders",
  "upscale_models",
  "vae",
];

interface FakeFolder {
  path: string;
  children: string[];
  readable?: boolean;
}

function installTree(root: string): FakeFolder[] {
  return [
    { path: root, children: [`${root}\\models`, `${root}\\custom_nodes`, `${root}\\output`] },
    { path: `${root}\\models`, children: MODEL_DIRS.map((d) => `${root}\\models\\${d}`) },
    ...MODEL_DIRS.map((d) => ({ path: `${root}\\models\\${d}`, children: [] as string[] })),
    { path: `${root}\\custom_nodes`, children: [] as string[] },
    { path: `${root}\\output`, children: [] as string[] },
  ];
}

/**
 * What an unregistered folder's extra_model_paths.yaml holds. A person adding
 * an install meets its yaml for the first time here, complaints and all.
 */
const YAML_ON_DISK: Record<string, ExtraPath[]> = {
  "C:\\ComfyUI-Portable": [
    {
      section: "comfyui",
      category: "loras",
      rawCategory: "loras",
      path: "D:\\ai-models\\loras",
      isDefault: false,
      exists: true,
    },
    {
      section: "comfyui",
      category: "..\\..\\ESCAPED",
      rawCategory: "..\\..\\ESCAPED",
      path: "D:\\ai-models\\spare",
      isDefault: false,
      exists: true,
    },
  ],
};

const DISK: FakeFolder[] = [
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
  ...installTree("C:\\ComfyUI-Alpha"),
  ...installTree("C:\\ComfyUI-Beta"),
  ...installTree("C:\\ComfyUI-Portable"),
  { path: "C:\\ComfyVault", children: MODEL_DIRS.map((d) => `C:\\ComfyVault\\${d}`) },
  ...MODEL_DIRS.map((d) => ({ path: `C:\\ComfyVault\\${d}`, children: [] as string[] })),
  { path: "C:\\Program Files", children: [], readable: false },
  { path: "C:\\Users", children: ["C:\\Users\\alex"] },
  { path: "C:\\Users\\alex", children: ["C:\\Users\\alex\\Downloads", "C:\\Users\\alex\\Documents"] },
  { path: "C:\\Users\\alex\\Downloads", children: [] },
  { path: "C:\\Users\\alex\\Documents", children: [] },
  { path: "D:\\", children: ["D:\\ai-models", "D:\\ComfyUI-Backup"] },
  { path: "E:\\", children: ["E:\\Backups"] },
  { path: "E:\\Backups", children: [] },
  // Listed, because the operating system lists it. Opening it fails.
  { path: "Z:\\", children: [], readable: false },
  { path: "D:\\ai-models", children: ["D:\\ai-models\\ltx"] },
  { path: "D:\\ai-models\\ltx", children: [] },
  ...installTree("D:\\ComfyUI-Backup"),
];

/** What a peek into a folder that is not registered turns up. */
const PEEK: Record<string, { files: number; bytes: number }> = {
  "C:\\ComfyUI-Portable": { files: 31, bytes: 118000 * MB },
  "D:\\ComfyUI-Backup": { files: 22, bytes: 96000 * MB },
};

/** The paths a running ComfyUI holds open. */
const LOCKED_PATHS = new Set(
  buildWorld()
    .contents.flatMap((c) => c.copies)
    .filter((c) => c.blocked === "fileLocked")
    .map((c) => c.absPath),
);

// ── events ──────────────────────────────────────────────────────────────────

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

// ── the engine ──────────────────────────────────────────────────────────────

export class FixtureEngine implements Engine {
  private world: World = buildWorld();
  private disk: FakeFolder[] = DISK.map((f) => ({ ...f, children: [...f.children] }));
  private vaultOpen = true;
  private driveReadable = true;

  /**
   * Every command but `get_app_state` refuses before a vault is chosen, and
   * the real engine has a test naming each one. The double refuses in the same
   * places, because a double that answers where the engine refuses is how a
   * first run reaches a person as a failure with a whole suite still green.
   */
  private requireVault(): void {
    if (!this.vaultOpen) {
      throw error(
        "notInitialized",
        "No vault folder is open yet. Choose a vault folder to continue.",
      );
    }
  }
  private lastScan: ScanRecord | null = null;
  private plans = new Map<string, ConsolidationPlan>();
  private applies: ApplyRecord[] = [];
  private worldBeforeApply: World | null = null;
  private busy: AppState["busy"] = null;

  private scanTimer: ReturnType<typeof setInterval> | null = null;
  private applyTimer: ReturnType<typeof setInterval> | null = null;
  private applyCancelling = false;
  /** Vault files this run created that were renamed after it finished. */
  private renamedSinceApply = new Set<string>();

  private scanProgressEvent = new Emitter<ScanProgress>();
  private scanDoneEvent = new Emitter<ScanRecord>();
  private scanErrorEvent = new Emitter<VaultError>();
  private applyProgressEvent = new Emitter<ApplyProgress>();
  private applyDoneEvent = new Emitter<ApplyRecord>();
  private applyErrorEvent = new Emitter<VaultError>();
  private revertProgressEvent = new Emitter<ApplyProgress>();
  private revertDoneEvent = new Emitter<ApplyRecord>();
  private revertErrorEvent = new Emitter<VaultError>();

  /** Everything is this many times faster. Tests pass a large number. */
  private readonly speed: number;
  private readonly manual: boolean;
  /**
   * One tick of whatever is running, or null when nothing is. In manual mode
   * the test calls it, so how far a run got is a decision the test makes
   * rather than a race with the machine it happens to be on.
   */
  private tick: (() => void) | null = null;

  constructor(
    options: { empty?: boolean; speed?: number; manual?: boolean } = {},
  ) {
    this.speed = options.speed ?? 1;
    this.manual = options.manual === true;
    if (options.empty === true) {
      this.emptyWorld();
      // A person who has never opened this program has no vault either, and
      // the engine refuses everything but get_app_state until they choose one.
      this.vaultOpen = false;
    }
    else this.recordScan("scan-1");
  }

  private get tickMs(): number {
    return Math.max(1, Math.round(TICK_MS / this.speed));
  }

  private emptyWorld(): void {
    this.world.installs = [];
    this.world.contents = [];
    this.world.vault.clear();
    this.world.running = [];
    this.lastScan = null;
  }

  private recordScan(scanId: string): ScanRecord {
    const result = scanResultOf(this.world, scanId);
    this.lastScan = result;
    this.world.installs = this.world.installs.map((install) => ({
      ...install,
      lastScanAt: result.finishedAt,
      lastScanTotals:
        result.perInstall.find((p) => p.installId === install.id) ?? null,
    }));
    return result;
  }

  // ── platform and state ────────────────────────────────────────────────────

  async getPlatformReport(): Promise<PlatformReport> {
    const supported = this.world.symlinksSupported;
    return {
      os: "windows",
      symlinks: {
        supported,
        probeError: supported
          ? null
          : "A required privilege is not held by the client. (os error 1314)",
        developerMode: supported,
        elevated: false,
        guidance: supported ? null : SYMLINK_GUIDANCE,
      },
      longPathsEnabled: true,
    };
  }

  async getAppState(): Promise<AppState> {
    return {
      vaultRoot: this.vaultOpen ? VAULT_ROOT : null,
      vaultInitialized: this.vaultOpen,
      installCount: this.world.installs.length,
      platform: await this.getPlatformReport(),
      settings: this.settingsNow(),
      lastScanId: this.lastScan?.scanId ?? null,
      lastPlanId: [...this.plans.keys()].at(-1) ?? null,
      interruptedApplies: this.applies
        .filter((a) => a.state === "interrupted")
        .map((a) => a.applyId),
      busy: this.busy,
    };
  }

  async selectVault(path: string): Promise<VaultInfo> {
    this.vaultOpen = true;
    const stored = [...this.world.vault.keys()].reduce(
      (sum, sha) =>
        sum + (this.world.contents.find((c) => c.sha256 === sha)?.bytes ?? 0),
      0,
    );
    return {
      root: path,
      createdAt: "2026-09-11T10:06:00.000Z",
      // What Windows answers: the drive root, with its trailing separator.
      // The engine has a test pinning that, and reporting a tidier "C:" here
      // hid a comparison that treated every install as being on another drive.
      volume: `${VAULT_VOLUME}\\`,
      // Null together when the drive cannot be read, never zero: zero of zero
      // reads as a completely full drive, which is a different statement.
      freeBytes: this.driveReadable ? this.world.freeBytes : null,
      totalBytes: this.driveReadable ? VAULT_TOTAL_BYTES : null,
      fileCount: this.world.vault.size,
      totalStoredBytes: stored,
      schemaVersion: 1,
    };
  }

  /**
   * Both drives this fake computer has, of the kinds a real one reports. It
   * answers before a vault exists, because it is what the first screen shows
   * while asking where the vault should go.
   */
  async listDrives(): Promise<DriveInfo[]> {
    return [
      { root: "C:\\", kind: "fixed", freeBytes: this.world.freeBytes, totalBytes: VAULT_TOTAL_BYTES },
      { root: "D:\\", kind: "fixed", freeBytes: 442_381_631_488, totalBytes: 1_024_209_543_168 },
      { root: "E:\\", kind: "removable", freeBytes: 3_199_827_968_000, totalBytes: 4_000_787_030_016 },
      // A network drive that has stopped answering. It is there, and it cannot
      // say how big it is, which is a different thing from being empty.
      { root: "Z:\\", kind: "network", freeBytes: null, totalBytes: null },
    ];
  }

  async getVaultInfo(): Promise<VaultInfo> {
    this.requireVault();
    return this.selectVault(VAULT_ROOT);
  }

  async getSettings(): Promise<Settings> {
    this.requireVault();
    return this.settingsNow();
  }

  /**
   * The settings as they stand. `get_app_state` reads them without a vault,
   * where the engine hands back its defaults, and it is the one command that
   * answers before a vault is chosen.
   */
  private settingsNow(): Settings {
    return {
      metadataLookupsEnabled: this.vaultOpen
        ? this.world.metadataLookupsEnabled
        : true,
      hashCacheEnabled: true,
      scanExtensions: [
        ".safetensors", ".ckpt", ".pt", ".pth", ".bin",
        ".gguf", ".onnx", ".pt2", ".sft", ".pkl",
      ],
      minFileSizeBytes: 1048576,
      followExtraModelPaths: true,
      scanOutputModelDirs: true,
      huggingFaceCacheDirs: null,
      verifyBeforeDelete: this.vaultOpen ? this.world.verifyBeforeDelete : true,
    };
  }

  async updateSettings(patch: Partial<Settings>): Promise<Settings> {
    this.requireVault();
    if (patch.metadataLookupsEnabled !== undefined) {
      this.world.metadataLookupsEnabled = patch.metadataLookupsEnabled;
    }
    if (patch.verifyBeforeDelete !== undefined) {
      this.world.verifyBeforeDelete = patch.verifyBeforeDelete;
    }
    return this.getSettings();
  }

  // ── installs ──────────────────────────────────────────────────────────────

  async validateInstallPath(path: string): Promise<InstallCandidate> {
    const folder = this.disk.find((f) => f.path === path);
    if (!folder || folder.readable === false) {
      return invalidCandidate("ComfyVault could not read that folder.");
    }
    if (!folder.children.some((c) => leafOf(c).toLowerCase() === "models")) {
      return invalidCandidate("No models folder was found inside that folder.");
    }
    // An install this world already knows carries its real yaml, complaints
    // and all, so the picker shows what registering it will really read.
    const known = this.world.installs.find(
      (i) => i.root.toLowerCase() === path.toLowerCase(),
    );
    const extraPaths = known?.extraPaths ?? YAML_ON_DISK[path] ?? [];
    return {
      valid: true,
      root: path,
      nestedDepth: 0,
      markersFound: [
        "main.py", "nodes.py", "folder_paths.py", "execution.py",
        "server.py", "comfy/", "comfy_extras/",
      ],
      markersMissing: [],
      contentCheckPassed: true,
      otherCandidates: [],
      version: known?.version ?? "0.29.1",
      versionSource: known?.versionSource ?? "comfyui_version.py",
      modelsDir: `${path}\\models`,
      modelsDirExists: true,
      extraPathsFile: extraPaths.length > 0 ? `${path}\\extra_model_paths.yaml` : null,
      extraPaths: extraPaths.filter((e) => !refusedCategory(e.rawCategory)),
      extraPathsProblems: extraPaths
        .filter((e) => refusedCategory(e.rawCategory))
        .map(
          (e) =>
            `In section "${e.section}", "${e.rawCategory}" cannot be a model folder name, so it was skipped. A model folder name cannot contain a path separator.`,
        ),
      outputModelDirs: [],
      reason: null,
    };
  }

  /** What the picker shows about a folder it has not registered yet. */
  peekAt(path: string): { files: number; bytes: number } {
    return PEEK[path] ?? { files: 147, bytes: 1044000 * MB };
  }

  async registerInstall(path: string, label?: string): Promise<Install> {
    this.requireVault();
    const candidate = await this.validateInstallPath(path);
    if (!candidate.valid) {
      throw error("notAComfyInstall", candidate.reason ?? "Not a ComfyUI install.");
    }
    if (this.world.installs.some((i) => i.root.toLowerCase() === path.toLowerCase())) {
      throw error("alreadyRegistered", "This install is registered already.");
    }
    const install: Install = {
      id: leafOf(path).toLowerCase().replace(/[^a-z0-9]+/g, "-"),
      label: label ?? (leafOf(path).replace(/^ComfyUI-?/i, "") || leafOf(path)),
      registeredPath: path,
      root: path,
      modelsDir: `${path}\\models`,
      version: candidate.version,
      versionSource: candidate.versionSource,
      extraPaths: candidate.extraPaths,
      outputModelDirs: candidate.outputModelDirs,
      addedAt: new Date().toISOString(),
      lastScanAt: null,
      lastScanTotals: null,
    };
    this.world.installs = [...this.world.installs, install];
    return install;
  }

  async listInstalls(): Promise<Install[]> {
    this.requireVault();
    return this.world.installs;
  }

  async refreshInstall(id: string): Promise<Install> {
    const install = this.world.installs.find((i) => i.id === id);
    if (!install) throw error("notFound", "That install is not registered.");
    return install;
  }

  async updateInstall(id: string, label: string): Promise<Install> {
    this.world.installs = this.world.installs.map((i) =>
      i.id === id ? { ...i, label } : i,
    );
    return this.refreshInstall(id);
  }

  async unregisterInstall(
    id: string,
  ): Promise<{ removed: true; linksLeftInPlace: number }> {
    const linksLeftInPlace = this.world.links.filter((l) => l.installId === id).length;
    this.world.installs = this.world.installs.filter((i) => i.id !== id);
    this.world.contents = this.world.contents.map((c) => ({
      ...c,
      copies: c.copies.filter((copy) => copy.installId !== id),
    }));
    this.world.links = this.world.links.filter((l) => l.installId !== id);
    this.recordScan(this.lastScan?.scanId ?? "scan-1");
    return { removed: true, linksLeftInPlace };
  }

  async listInstallModelDirs(id: string): Promise<ModelDirNode[]> {
    const install = this.world.installs.find((i) => i.id === id);
    if (!install) throw error("notFound", "That install is not registered.");
    return [
      {
        relPath: "models",
        absPath: install.modelsDir,
        category: "",
        origin: "modelsDir",
        fileCount: 0,
        children: MODEL_DIRS.map((d) => ({
          relPath: `models/${d}`,
          absPath: `${install.modelsDir}\\${d}`,
          category: d,
          origin: "modelsDir" as const,
          fileCount: 0,
          children: [],
        })),
      },
    ];
  }

  // ── directories, for the folder picker ────────────────────────────────────

  async listDirectory(path: string | null): Promise<DirectoryListing> {
    if (path === null) {
      return {
        path: "",
        parent: null,
        entries: this.disk
          .filter((f) => /^[A-Za-z]:\\$/.test(f.path))
          .map((f) => ({
            name: f.path,
            path: f.path,
            isDirectory: true,
            isSymlink: false,
          })),
      };
    }
    const folder = this.disk.find((f) => f.path === path);
    if (!folder) {
      throw { ...error("notFound", "That folder is not there any more."), path };
    }
    // A folder that cannot be read is a refusal carrying the path, never an
    // empty listing, so the picker can say why rather than look empty.
    if (folder.readable === false) {
      throw {
        ...error("permissionDenied", "Windows will not let ComfyVault open that folder."),
        path,
      };
    }
    return {
      path,
      parent: parentOf(path),
      entries: folder.children.map((child) => ({
        name: leafOf(child),
        path: child,
        isDirectory: true,
        isSymlink: false,
      })),
    };
  }

  async createDirectory(path: string): Promise<{ path: string; created: boolean }> {
    if (this.disk.some((f) => f.path.toLowerCase() === path.toLowerCase())) {
      return { path, created: false };
    }
    const parent = parentOf(path);
    const folder = parent ? this.disk.find((f) => f.path === parent) : undefined;
    if (!folder) {
      throw { ...error("notFound", "That folder is not there any more."), path };
    }
    if (folder.readable === false) {
      throw {
        ...error("permissionDenied", "Windows refused to write there."),
        path,
      };
    }
    folder.children.push(path);
    this.disk.push({ path, children: [] });
    return { path, created: true };
  }

  // ── scan ──────────────────────────────────────────────────────────────────

  async startScan(): Promise<{ scanId: string }> {
    this.requireVault();
    if (this.busy) throw error("vaultBusy", "Something is already running.");
    const scanId = `scan-${Date.now()}`;
    this.busy = { kind: "scan", id: scanId };
    const entries = scanEntriesOf(this.world);
    const filesToHash = entries.length;
    const bytesToHash = entries.reduce((s, e) => s + e.sizeBytes, 0);
    // Simulated time, one TICK_MS per step. A run takes the same number of
    // steps whatever the machine and whatever the speed, so nothing about it
    // depends on how long a step took in the real world.
    let elapsed = 0;

    const step = () => {
      elapsed += TICK_MS;
      const overall = Math.min(1, elapsed / SCAN_MS);
      const phase: ScanProgress["phase"] =
        overall < 0.2 ? "enumerating" : overall < 0.95 ? "hashing" : "finalizing";
      const hashed = Math.max(0, Math.min(1, (overall - 0.2) / 0.75));
      const at = Math.min(entries.length - 1, Math.floor(hashed * entries.length));
      const entry = entries[at];

      this.scanProgressEvent.emit({
        scanId,
        phase,
        installId: entry?.installId ?? null,
        installLabel:
          this.world.installs.find((i) => i.id === entry?.installId)?.label ?? null,
        filesSeen:
          phase === "enumerating"
            ? Math.round(filesToHash * (overall / 0.2))
            : filesToHash + COUNTED_NEVER_MOVED.customNodeFiles +
              COUNTED_NEVER_MOVED.hfCacheFiles,
        filesToHash,
        filesHashed: Math.round(filesToHash * hashed),
        bytesToHash,
        bytesHashed: Math.round(bytesToHash * hashed),
        bytesFromCache: 0,
        currentPath: phase === "hashing" ? (entry?.absPath ?? null) : null,
        elapsedMs: elapsed,
        etaMs: Math.max(0, SCAN_MS - elapsed),
      });

      if (overall >= 1) {
        this.stopScanTimer();
        this.busy = null;
        this.scanDoneEvent.emit(this.recordScan(scanId));
      }
    };
    this.tick = step;
    if (!this.manual) this.scanTimer = setInterval(step, this.tickMs);

    return { scanId };
  }

  async cancelScan(scanId: string): Promise<{ cancelled: true }> {
    this.stopScanTimer();
    this.busy = null;
    this.scanDoneEvent.emit(scanResultOf(this.world, scanId, true));
    return { cancelled: true };
  }

  private stopScanTimer(): void {
    if (this.scanTimer) clearInterval(this.scanTimer);
    this.scanTimer = null;
    this.tick = null;
  }

  async getLastScan(): Promise<ScanRecord | null> {
    this.requireVault();
    return this.lastScan;
  }

  async getScanEntries(args: {
    scanId: string;
    offset: number;
    limit: number;
  }): Promise<ScanEntryPage> {
    this.requireVault();
    const all = scanEntriesOf(this.world);
    return {
      total: all.length,
      offset: args.offset,
      entries: all.slice(args.offset, args.offset + Math.min(args.limit, 1000)),
    };
  }

  onScanProgress(fn: (p: ScanProgress) => void): Unsubscribe {
    return this.scanProgressEvent.on(fn);
  }
  onScanDone(fn: (r: ScanRecord) => void): Unsubscribe {
    return this.scanDoneEvent.on(fn);
  }
  onScanError(fn: (e: VaultError) => void): Unsubscribe {
    return this.scanErrorEvent.on(fn);
  }

  // ── plan ──────────────────────────────────────────────────────────────────

  async buildPlan(scanId: string): Promise<ConsolidationPlan> {
    this.requireVault();
    const planId = `plan-${scanId}-${this.world.symlinksSupported ? "links" : "nolinks"}-${this.world.vault.size}`;
    const plan = planOf(this.world, planId, scanId);
    this.plans.set(planId, plan);
    return plan;
  }

  async getPlan(planId: string): Promise<ConsolidationPlan> {
    const plan = this.plans.get(planId);
    if (!plan) throw error("notFound", "That plan is gone. Run the scan again.");
    return plan;
  }

  // ── apply ─────────────────────────────────────────────────────────────────

  async startApply(args: {
    planId: string;
    groupIds: string[];
  }): Promise<{ applyId: string }> {
    this.requireVault();
    if (this.busy) throw error("vaultBusy", "Something is already running.");
    const plan = await this.getPlan(args.planId);
    const groups = plan.groups.filter((g) => args.groupIds.includes(g.groupId));
    const applyId = `apply-${Date.now()}`;
    this.busy = { kind: "apply", id: applyId };
    this.applyCancelling = false;
    this.worldBeforeApply = cloneWorld(this.world);
    this.renamedSinceApply.clear();
    // Read from the drive before anything moves, the way the engine reads it.
    const freeBefore = this.world.freeBytes;

    const bytesToMove = groups.reduce((s, g) => s + g.sizeBytes, 0);
    const started = Date.now();
    // One file is written to between the report and the run, as really happens.
    const changesAt = groups.length > 7 ? 6 : -1;
    let reached = 0;
    let elapsed = 0;

    const step = () => {
      elapsed += TICK_MS;
      // A cancel stops where the run is. The group it was part way through is
      // undone, which here means it is simply never committed, and no group
      // after it is started.
      const overall = this.applyCancelling
        ? 1
        : Math.min(1, elapsed / APPLY_MS);
      const upto = this.applyCancelling
        ? reached
        : Math.min(groups.length, Math.floor(overall * groups.length));

      while (reached < upto) {
        const group = groups[reached]!;
        if (reached !== changesAt) this.commitGroup(group);
        reached += 1;
      }

      const done = groups.slice(0, reached).filter((_, i) => i !== changesAt);
      const current = groups[Math.min(groups.length - 1, reached)];
      this.applyProgressEvent.emit({
        applyId,
        phase: overall < 0.02 ? "preflight" : overall >= 1 ? "finalizing" : "applying",
        groupIndex: reached,
        groupTotal: groups.length,
        currentGroupId: current?.groupId ?? null,
        currentPath: current?.source.absPath ?? null,
        step: "moving",
        bytesMoved: done.reduce((s, g) => s + g.sizeBytes, 0),
        bytesToMove,
        bytesFreed: done.reduce((s, g) => s + g.bytesFreed, 0),
        filesMoved: done.length,
        linksCreated: done.reduce((s, g) => s + g.occurrences, 0),
        failures: changesAt >= 0 && reached > changesAt ? 1 : 0,
        elapsedMs: elapsed,
        etaMs: Math.max(0, APPLY_MS - elapsed),
      });

      if (overall >= 1) {
        this.stopApplyTimer();
        this.busy = null;
        // Something else on the computer wrote while the run was going. A real
        // drive does this, and it is why the free space before and after are
        // read rather than worked out from what the run removed.
        this.world.freeBytes -= OTHER_ACTIVITY_BYTES;
        // The group a cancel interrupts is rolled back, not failed: the person
        // asked for it to stop. A group that genuinely failed before they
        // pressed stop is still reported, because nothing else mentions it.
        const cancelled = this.applyCancelling;
        const failed = changesAt >= 0 && reached > changesAt ? groups[changesAt] : undefined;
        const result: ApplyRecord = {
          applyId,
          planId: args.planId,
          groupIds: args.groupIds,
          state: cancelled ? "cancelled" : failed ? "completedWithErrors" : "completed",
          startedAt: new Date(started).toISOString(),
          finishedAt: new Date().toISOString(),
          groupsRequested: groups.length,
          groupsApplied: done.length,
          groupsFailed: failed ? 1 : 0,
          // What this run removed, summed from the groups it finished, the
          // way the engine sums it. Not the change in free space: those are
          // different numbers and saying so is the point of the two below.
          bytesFreed: done.reduce((sum, g) => sum + g.bytesFreed, 0),
          vaultFreeBytesBefore: freeBefore,
          vaultFreeBytesAfter: this.world.freeBytes,
          filesMoved: done.length,
          linksCreated: done.reduce((s, g) => s + g.occurrences, 0),
          failures: failed
            ? [
                {
                  groupId: failed.groupId,
                  absPath: failed.source.absPath,
                  reason: "fileChanged",
                  detail:
                    "The size or modification time no longer matches what the scan recorded.",
                },
              ]
            : [],
          revertible: true,
        };
        this.applies = [result, ...this.applies];
        this.recordScan(this.lastScan?.scanId ?? "scan-1");
        this.applyDoneEvent.emit(result);
      }
    };
    this.tick = step;
    if (!this.manual) this.applyTimer = setInterval(step, this.tickMs);

    return { applyId };
  }

  /** Move the source into the vault and leave a link at every path it covered. */
  private commitGroup(group: PlanGroup): void {
    const content = this.world.contents.find((c) => c.sha256 === group.sha256);
    if (!content) return;
    const vaultName = fileNameOf(group.vaultRelPath);
    const entry = this.world.vault.get(group.sha256) ?? {
      sha256: group.sha256,
      canonicalName: vaultName,
      aliases: [],
      addedAt: new Date().toISOString(),
    };
    for (const link of group.links) {
      if (
        link.linkName !== entry.canonicalName &&
        !entry.aliases.includes(link.linkName)
      ) {
        entry.aliases.push(link.linkName);
      }
    }
    this.world.vault.set(group.sha256, entry);

    for (const copy of content.copies) {
      if (copy.blocked || copy.isLink) continue;
      if (!group.links.some((l) => l.absPath === copy.absPath)) continue;
      copy.isLink = true;
      this.world.links.push({
        id: `link-${this.world.links.length + 1}`,
        installId: copy.installId,
        absPath: copy.absPath,
        relPath: copy.relPath,
        linkName: copy.name,
        sha256: group.sha256,
        vaultRelPath: group.vaultRelPath,
        createdAt: new Date().toISOString(),
        createdBy: "apply",
        applyId: this.busy?.id ?? null,
      });
    }
    this.world.freeBytes += group.bytesFreed;
  }

  async cancelApply(): Promise<{ cancelled: true }> {
    this.applyCancelling = true;
    return { cancelled: true };
  }

  private stopApplyTimer(): void {
    if (this.applyTimer) clearInterval(this.applyTimer);
    this.applyTimer = null;
    this.tick = null;
  }

  async getApplyResult(applyId: string): Promise<ApplyRecord> {
    const result = this.applies.find((a) => a.applyId === applyId);
    if (!result) throw error("notFound", "That run is not on record.");
    return result;
  }

  async listApplies(): Promise<ApplyRecord[]> {
    this.requireVault();
    return this.applies;
  }

  async getInterruptedApplies(): Promise<InterruptedApply[]> {
    this.requireVault();
    return this.applies
      .filter((a) => a.state === "interrupted")
      .map((a) => ({
        applyId: a.applyId,
        planId: a.planId,
        startedAt: a.startedAt,
        stepsDone: a.groupsApplied,
        stepsPending: a.groupsRequested - a.groupsApplied,
        description:
          "ComfyVault stopped part way through a run. Finishing it puts every remaining file where the plan said.",
        affectedPaths: [],
      }));
  }

  async resumeApply(applyId: string): Promise<{ applyId: string }> {
    return { applyId };
  }

  async revertApply(applyId: string): Promise<{ applyId: string }> {
    const before = this.worldBeforeApply;
    if (!before) throw error("conflict", "There is nothing to put back.");
    if (this.renamedSinceApply.size > 0) {
      const paths = vaultFilesOf(this.world)
        .filter((f) => this.renamedSinceApply.has(f.sha256))
        .map((f) => `${VAULT_ROOT}\\${f.vaultRelPath}`);
      throw error(
        "conflict",
        "Files this run created have been renamed since, so putting them back would lose the new names.",
        paths.join("\n"),
      );
    }
    this.world = before;
    this.worldBeforeApply = null;
    this.renamedSinceApply.clear();
    this.applies = this.applies.map((a) =>
      a.applyId === applyId ? { ...a, state: "reverted" as const, revertible: false } : a,
    );
    this.recordScan(this.lastScan?.scanId ?? "scan-1");
    this.revertDoneEvent.emit(await this.getApplyResult(applyId));
    return { applyId };
  }

  onApplyProgress(fn: (p: ApplyProgress) => void): Unsubscribe {
    return this.applyProgressEvent.on(fn);
  }
  onApplyDone(fn: (r: ApplyRecord) => void): Unsubscribe {
    return this.applyDoneEvent.on(fn);
  }
  onApplyError(fn: (e: VaultError) => void): Unsubscribe {
    return this.applyErrorEvent.on(fn);
  }
  onRevertProgress(fn: (p: ApplyProgress) => void): Unsubscribe {
    return this.revertProgressEvent.on(fn);
  }
  onRevertDone(fn: (r: ApplyRecord) => void): Unsubscribe {
    return this.revertDoneEvent.on(fn);
  }
  onRevertError(fn: (e: VaultError) => void): Unsubscribe {
    return this.revertErrorEvent.on(fn);
  }

  // ── links ─────────────────────────────────────────────────────────────────

  async createLink(args: {
    installId: string;
    sha256: string;
    relativeDir: string;
    linkName?: string;
  }): Promise<LinkRecord> {
    const install = this.world.installs.find((i) => i.id === args.installId);
    if (!install) throw error("notFound", "That install is not registered.");
    const entry = this.world.vault.get(args.sha256);
    if (!entry) throw error("notFound", "The vault does not hold that file.");
    const linkName = args.linkName ?? entry.canonicalName;
    const absPath = `${install.root}\\${args.relativeDir}\\${linkName}`;
    if (this.world.links.some((l) => l.absPath === absPath)) {
      throw error("conflict", "Something already sits at that name.");
    }
    const link: LinkRecord = {
      id: `link-${this.world.links.length + 1}`,
      installId: args.installId,
      absPath,
      relPath: `${args.relativeDir}\\${linkName}`,
      linkName,
      sha256: args.sha256,
      vaultRelPath: entry.canonicalName,
      createdAt: new Date().toISOString(),
      createdBy: "manual",
      applyId: null,
    };
    this.world.links.push(link);
    return link;
  }

  async removeLink(linkId: string): Promise<{ removed: true }> {
    this.world.links = this.world.links.filter((l) => l.id !== linkId);
    return { removed: true };
  }

  async createModelFolder(
    installId: string,
    relativeDir: string,
  ): Promise<{ absPath: string; created: boolean }> {
    const install = this.world.installs.find((i) => i.id === installId);
    if (!install) throw error("notFound", "That install is not registered.");
    return { absPath: `${install.root}\\${relativeDir}`, created: true };
  }

  async listLinks(filter?: { sha256?: string }): Promise<LinkWithState[]> {
    this.requireVault();
    const links = filter?.sha256
      ? this.world.links.filter((l) => l.sha256 === filter.sha256)
      : this.world.links;
    // State is a fact about the drive, read now rather than remembered.
    return links.map((link) => ({ ...link, state: this.stateOf(link) }));
  }

  /** What is actually at a link's path this moment. */
  private stateOf(link: LinkRecord): LinkState {
    if (!this.world.vault.has(link.sha256)) return "dangling";
    const content = this.world.contents.find((c) => c.sha256 === link.sha256);
    const copy = content?.copies.find((x) => x.absPath === link.absPath);
    if (copy === undefined) return "missing";
    return copy.isLink ? "ok" : "replaced";
  }

  // ── vault contents ────────────────────────────────────────────────────────

  async listVaultFiles(args: {
    offset: number;
    limit: number;
  }): Promise<VaultFilePage> {
    this.requireVault();
    const all = vaultFilesOf(this.world);
    return {
      total: all.length,
      offset: args.offset,
      files: all.slice(args.offset, args.offset + Math.min(args.limit, 1000)),
    };
  }

  async listContents(args: {
    offset: number;
    limit: number;
    filter?: ContentFilter;
    sort?: "name" | "size" | "addedAt" | "linkCount" | "occurrences";
    descending?: boolean;
  }): Promise<ContentPage> {
    this.requireVault();
    const filter = args.filter ?? {};
    let rows = contentRowsOf(this.world);
    if (filter.inVault !== undefined) {
      rows = rows.filter((r) => r.inVault === filter.inVault);
    }
    if (filter.category) rows = rows.filter((r) => r.category === filter.category);
    if (filter.minSizeBytes !== undefined) {
      rows = rows.filter((r) => r.sizeBytes >= filter.minSizeBytes!);
    }
    if (filter.nameContains) {
      const needle = filter.nameContains.toLowerCase();
      rows = rows.filter((r) =>
        [r.name, ...r.aliases].some((n) => n.toLowerCase().includes(needle)),
      );
    }
    if (filter.withAliasesOnly) rows = rows.filter((r) => r.aliases.length > 0);
    // A model still sitting in an install is not an orphan: it is there.
    if (filter.orphansOnly) {
      rows = rows.filter((r) => r.inVault && r.occurrenceCount === 0);
    }

    const descending = args.descending ?? true;
    const by = args.sort ?? "size";
    rows.sort((a, b) => {
      const order =
        by === "name"
          ? a.name.localeCompare(b.name)
          : by === "addedAt"
            ? (a.addedAt ?? "").localeCompare(b.addedAt ?? "")
            : by === "linkCount"
              ? a.linkCount - b.linkCount
              : by === "occurrences"
                ? a.occurrenceCount - b.occurrenceCount
                : a.sizeBytes - b.sizeBytes;
      return descending ? -order : order;
    });

    return {
      total: rows.length,
      offset: args.offset,
      rows: rows.slice(args.offset, args.offset + Math.min(args.limit, 1000)),
      scanId: this.lastScan?.scanId ?? null,
    };
  }

  async listNameGroups(): Promise<NameGroup[]> {
    this.requireVault();
    return nameGroupsOf(this.world);
  }

  async setCanonicalName(sha256: string, name: string): Promise<VaultFile> {
    const entry = this.world.vault.get(sha256);
    if (!entry) throw error("notFound", "The vault does not hold that file.");
    if (this.worldBeforeApply && !this.worldBeforeApply.vault.has(sha256)) {
      // This run put the file in the vault and it has been renamed since.
      this.renamedSinceApply.add(sha256);
    }
    const names = [entry.canonicalName, ...entry.aliases];
    if (!names.includes(name)) names.push(name);
    entry.canonicalName = name;
    entry.aliases = names.filter((n) => n !== name);
    const file = vaultFilesOf(this.world).find((f) => f.sha256 === sha256);
    if (!file) throw error("notFound", "The vault does not hold that file.");
    return file;
  }

  async removeAlias(sha256: string, name: string): Promise<{ removed: true }> {
    const entry = this.world.vault.get(sha256);
    if (!entry) throw error("notFound", "The vault does not hold that file.");
    if (entry.canonicalName === name) {
      throw error("conflict", "That is the name the vault keeps.");
    }
    const used = this.world.links.filter(
      (l) => l.sha256 === sha256 && l.linkName === name,
    );
    if (used.length > 0) {
      throw error(
        "conflict",
        `${used.length} ${used.length === 1 ? "link resolves" : "links resolve"} through that name.`,
      );
    }
    entry.aliases = entry.aliases.filter((n) => n !== name);
    return { removed: true };
  }

  async listOrphans(): Promise<VaultFile[]> {
    this.requireVault();
    return vaultFilesOf(this.world).filter((f) => f.linkCount === 0);
  }

  async deleteVaultFile(
    sha256: string,
    confirm: string,
  ): Promise<{ deleted: true; bytesFreed: number }> {
    if (confirm !== sha256) {
      throw error("invalidArgument", "The confirmation did not match.");
    }
    if (this.world.links.some((l) => l.sha256 === sha256)) {
      throw error("conflict", "An install still links to that file.");
    }
    const content = this.world.contents.find((c) => c.sha256 === sha256);
    this.world.vault.delete(sha256);
    const bytesFreed = content?.bytes ?? 0;
    this.world.freeBytes += bytesFreed;
    return { deleted: true, bytesFreed };
  }

  async checkVaultHealth(): Promise<VaultHealth> {
    this.requireVault();
    // A link dangles when the vault no longer holds what it points at. The
    // engine verifies each recorded link on disk; here the world is the disk.
    const danglingLinks = this.world.links.filter(
      (link) => !this.world.vault.has(link.sha256),
    );
    const replacedLinks = this.world.links.filter((link) => {
      const content = this.world.contents.find((c) => c.sha256 === link.sha256);
      const copy = content?.copies.find((x) => x.absPath === link.absPath);
      return copy !== undefined && !copy.isLink;
    });
    const missingVaultFiles = vaultFilesOf(this.world).filter((f) => !f.present);
    return {
      checkedLinks: this.world.links.length,
      checkedFiles: this.world.vault.size,
      danglingLinks,
      replacedLinks,
      missingVaultFiles,
      foreignFiles: [],
      ok:
        danglingLinks.length === 0 &&
        replacedLinks.length === 0 &&
        missingVaultFiles.length === 0,
    };
  }

  // ── usage and metadata ────────────────────────────────────────────────────

  async checkModelUsage(names: string[]): Promise<UsageResult[]> {
    this.requireVault();
    if (this.world.workflowsOnDisk === 0) {
      return names.map((name) => ({
        name,
        used: false,
        searched: false,
        matches: [],
        method: NOTHING_SEARCHED,
      }));
    }
    return names.map((name) => {
      const content = this.world.contents.find(
        (c) => c.filename === name || c.copies.some((copy) => copy.name === name),
      );
      const hits = content?.workflowHits ?? 0;
      return {
        name,
        used: hits > 0,
        searched: true,
        matches: Array.from({ length: Math.min(hits, 4) }, (_, i) => ({
          installId: "prod",
          installLabel: "Production",
          workflowPath: `C:\\ComfyUI-Alpha\\user\\default\\workflows\\flow-${i + 1}.json`,
          workflowName: `flow-${i + 1}.json`,
        })),
        method: USAGE_METHOD,
      };
    });
  }

  async getMetadata(sha256: string): Promise<ModelMetadata | null> {
    this.requireVault();
    if (!this.world.metadataLookupsEnabled) return null;
    return this.world.contents.find((c) => c.sha256 === sha256)?.metadata ?? null;
  }

  async fetchMetadataBatch(hashes: string[]): Promise<ModelMetadata[]> {
    this.requireVault();
    const out: ModelMetadata[] = [];
    for (const sha of hashes) {
      const found = await this.getMetadata(sha);
      if (found) out.push(found);
    }
    return out;
  }

  // ── running programs ──────────────────────────────────────────────────────

  async getRunningComfy(): Promise<RunningComfy[]> {
    this.requireVault();
    return this.world.running.map((installId, i) => {
      const root = this.world.installs.find((x) => x.id === installId)?.root ?? null;
      return {
        pid: 18244 + i,
        name: "python.exe",
        exePath: root ? `${root}\\python_embeded\\python.exe` : null,
        cwd: root,
        commandLine: ["python.exe", "main.py"],
        matchedInstallIds: [installId],
        matchReason: "exeUnderRoot" as const,
      };
    });
  }

  async checkLockedFiles(paths: string[]): Promise<LockState[]> {
    return paths.map((path) => ({
      path,
      locked: LOCKED_PATHS.has(path) && this.world.running.length > 0,
      checkable: true,
      detail: null,
    }));
  }

  // ── the window ────────────────────────────────────────────────────────────

  async openExternal(): Promise<void> {}
  async revealInFileManager(): Promise<void> {}
  async windowMinimize(): Promise<void> {}
  async windowToggleMaximize(): Promise<void> {}
  async windowClose(): Promise<void> {}

  // ── development affordances ───────────────────────────────────────────────

  /** Turn Windows Developer Mode on or off, which only Windows can really do. */
  devSetSymlinksSupported(on: boolean): void {
    this.world.symlinksSupported = on;
  }

  /** Close or start ComfyUI, which only the person can really do. */
  devSetComfyRunning(running: boolean): void {
    this.world.running = running ? ["prod"] : [];
    for (const content of this.world.contents) {
      for (const copy of content.copies) {
        if (copy.blocked === "fileLocked" && !running) copy.blocked = null;
        else if (copy.blocked === null && running && LOCKED_PATHS.has(copy.absPath)) {
          copy.blocked = "fileLocked";
        }
      }
    }
    this.recordScan(this.lastScan?.scanId ?? "scan-1");
  }

  /**
   * Take a file out of the vault from underneath its links, the way something
   * outside ComfyVault would. Every link to it then points at nothing.
   */
  devBreakLinks(count = 1): number {
    const broken = new Set<string>();
    for (const link of this.world.links) {
      if (broken.size >= count) break;
      broken.add(link.sha256);
    }
    for (const sha of broken) this.world.vault.delete(sha);
    return this.world.links.filter((l) => broken.has(l.sha256)).length;
  }

  /**
   * Advance a running scan or apply by whole steps.
   *
   * Only a manual engine has a clock to advance. A test that drives the run
   * itself is asserting about the rule, not about how fast the machine it runs
   * on happens to be.
   */
  devAdvance(steps = 1): void {
    if (!this.manual) {
      throw new Error("devAdvance needs a manual engine: new FixtureEngine({ manual: true })");
    }
    if (this.tick === null) throw new Error("devAdvance: nothing is running");
    for (let i = 0; i < steps; i += 1) {
      const step = this.tick;
      if (step === null) return;
      step();
    }
  }

  /**
   * Advance until whatever is running has finished.
   *
   * It refuses when nothing is running. A version that quietly did nothing
   * would let every test built on it pass while asserting about a world no run
   * ever touched, and it would pass silently, which is the worst way to be
   * wrong.
   */
  devFinish(limit = 5000): void {
    if (!this.manual) {
      throw new Error("devFinish needs a manual engine: new FixtureEngine({ manual: true })");
    }
    if (this.tick === null) throw new Error("devFinish: nothing is running");
    let steps = 0;
    for (; steps < limit; steps += 1) {
      const step = this.tick;
      if (step === null) return;
      step();
    }
    throw new Error(`devFinish: still running after ${limit} steps`);
  }

  /**
   * The vault's drive stops answering when asked how much room it has, which
   * is what a network drive or a pulled card reader does.
   */
  devSetDriveReadable(readable: boolean): void {
    this.driveReadable = readable;
  }

  /** Nobody ever pressed Save, so there is nothing on disk to search. */
  devSetWorkflowsOnDisk(count: number): void {
    this.world.workflowsOnDisk = count;
  }

  /** An install too old to record its version, which many really are. */
  devForgetVersions(): void {
    this.world.installs = this.world.installs.map((i) => ({
      ...i,
      version: null,
      versionSource: null,
    }));
  }

  devReset(empty: boolean): void {
    this.vaultOpen = !empty;
    this.stopScanTimer();
    this.stopApplyTimer();
    this.world = buildWorld();
    this.disk = DISK.map((f) => ({ ...f, children: [...f.children] }));
    this.plans.clear();
    this.applies = [];
    this.worldBeforeApply = null;
    this.busy = null;
    if (empty) this.emptyWorld();
    else this.recordScan("scan-1");
  }
}

function invalidCandidate(reason: string): InstallCandidate {
  return {
    valid: false,
    root: null,
    nestedDepth: 0,
    markersFound: [],
    markersMissing: ["main.py", "nodes.py", "folder_paths.py"],
    contentCheckPassed: false,
    otherCandidates: [],
    version: null,
    versionSource: null,
    modelsDir: null,
    modelsDirExists: false,
    extraPathsFile: null,
    extraPaths: [],
    extraPathsProblems: [],
    outputModelDirs: [],
    reason,
  };
}

/** "C:\\Users\\alex" -> "C:\\Users". A drive root has no parent. */
function parentOf(path: string): string | null {
  if (/^[A-Za-z]:\\$/.test(path)) return null;
  const trimmed = path.endsWith("\\") ? path.slice(0, -1) : path;
  const at = trimmed.lastIndexOf("\\");
  if (at < 0) return null;
  const parent = trimmed.slice(0, at);
  return /^[A-Za-z]:$/.test(parent) ? `${parent}\\` : parent;
}

/** A category name that would put a file outside the vault. */
function refusedCategory(rawCategory: string): boolean {
  return rawCategory.includes("\\") || rawCategory.includes("/");
}

function error(code: VaultError["code"], message: string, detail?: string): VaultError {
  return detail === undefined ? { code, message } : { code, message, detail };
}

function cloneWorld(world: World): World {
  return {
    ...world,
    installs: world.installs.map((i) => ({ ...i })),
    contents: world.contents.map((c) => ({
      ...c,
      copies: c.copies.map((copy) => ({ ...copy })),
    })),
    vault: new Map(
      [...world.vault.entries()].map(([k, v]) => [k, { ...v, aliases: [...v.aliases] }]),
    ),
    links: world.links.map((l) => ({ ...l })),
    running: [...world.running],
  };
}
