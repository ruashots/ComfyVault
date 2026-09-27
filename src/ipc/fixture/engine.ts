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
import { DownloadDesk } from "~/ipc/fixture/downloads";
import type {
  ApplyProgress,
  ApplyRecord,
  AppState,
  ConsolidationPlan,
  ContentFilter,
  ContentPage,
  Deleted,
  DirectoryListing,
  Download,
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
  HiddenNameCard,
  NameGroup,
  PlanGroup,
  PlatformReport,
  RevertPreview,
  RevertProgress,
  RunningComfy,
  ScanEntryPage,
  ScanProgress,
  ScanRecord,
  Settings,
  TokenService,
  Unsubscribe,
  UsageResult,
  VaultError,
  UnifyPlan,
  UnifyResult,
  UnifyLink,
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
const REVERT_MS = 12_000;
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
  /** It holds files as well as the folders listed, which the picker never lists. */
  hasFiles?: boolean;
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
      "C:\\ComfyUI-Studio",
      "C:\\ComfyUI-Sandbox",
      "C:\\ComfyUI-Portable",
      "C:\\ComfyVault",
      "C:\\Program Files",
      "C:\\Users",
    ],
  },
  ...installTree("C:\\ComfyUI-Studio"),
  ...installTree("C:\\ComfyUI-Sandbox"),
  ...installTree("C:\\ComfyUI-Portable"),
  { path: "C:\\ComfyVault", children: MODEL_DIRS.map((d) => `C:\\ComfyVault\\${d}`) },
  ...MODEL_DIRS.map((d) => ({ path: `C:\\ComfyVault\\${d}`, children: [] as string[] })),
  { path: "C:\\Program Files", children: [], readable: false },
  { path: "C:\\Users", children: ["C:\\Users\\alex"] },
  { path: "C:\\Users\\alex", children: ["C:\\Users\\alex\\Downloads", "C:\\Users\\alex\\Documents"] },
  { path: "C:\\Users\\alex\\Downloads", children: [] },
  { path: "C:\\Users\\alex\\Documents", children: [], hasFiles: true },
  { path: "D:\\", children: ["D:\\AI", "D:\\ai-models", "D:\\ComfyUI-Backup"] },
  // Three installs under one folder that is not an install itself.
  { path: "D:\\AI", children: ["D:\\AI\\ComfyUI-Flux", "D:\\AI\\ComfyUI-SDXL", "D:\\AI\\ComfyUI-Video"] },
  ...installTree("D:\\AI\\ComfyUI-Flux"),
  ...installTree("D:\\AI\\ComfyUI-SDXL"),
  ...installTree("D:\\AI\\ComfyUI-Video"),
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
  /** When the latest scan was cancelled; null once a later one finishes. */
  private lastCancelledScanAt: string | null = null;
  private plans = new Map<string, ConsolidationPlan>();
  private applies: ApplyRecord[] = [];
  private worldBeforeApply: World | null = null;
  private busy: AppState["busy"] = null;

  private scanTimer: ReturnType<typeof setInterval> | null = null;
  private applyTimer: ReturnType<typeof setInterval> | null = null;
  private applyCancelling = false;
  /**
   * The room an undo's copies take, when it is not their size. A sparse or
   * compressed model occupies far less than its size, and so does its copy.
   */
  private revertRoomBytes: number | null = null;
  /** Cut-off runs that name places outside the vault and the installs. */
  private blockedRuns = new Map<string, string[]>();
  /** The engine's metadata cache. It lives in the vault, so an undo keeps it. */
  private metadataCache = new Map<string, ModelMetadata>();
  private civitaiReachable = true;
  private civitaiRequests = 0;
  /** Set by `devCutOffApply`: the next tick ends the run where it is. */
  private cuttingOff = false;
  /** Runs cut off part way, with what they did and what they still owe. */
  private cutOffRuns = new Map<
    string,
    { done: PlanGroup[]; pending: PlanGroup[]; freeBefore: number }
  >();
  /** How many of each run's undo steps are done, kept across a stopped undo. */
  private revertStepsDone = new Map<string, number>();
  /** The groups each run finished, which are what undoing it puts back. */
  private appliedGroups = new Map<string, PlanGroup[]>();
  /** Vault files this run created that were renamed after it finished. */
  private renamedSinceApply = new Set<string>();
  /** Places in the installs that hold some other file, lower-cased. */
  private takenPaths = new Set<string>();
  /** The name cards the person kept as they are. Kept in the app config. */
  private hiddenNameCards: HiddenNameCard[] = [];
  private unifyCount = 0;
  /** Paths of models this run put in the vault that Cleanup deleted since. */
  private deletedSinceApply: string[] = [];
  /** Links a delete removed before it was cut off, by the model's SHA-256. */
  private stoppedDeletes = new Map<string, string[]>();

  private scanProgressEvent = new Emitter<ScanProgress>();
  private scanDoneEvent = new Emitter<ScanRecord>();
  private scanErrorEvent = new Emitter<VaultError>();
  private applyProgressEvent = new Emitter<ApplyProgress>();
  private applyDoneEvent = new Emitter<ApplyRecord>();
  private applyErrorEvent = new Emitter<VaultError>();
  private revertProgressEvent = new Emitter<RevertProgress>();
  private revertDoneEvent = new Emitter<ApplyRecord>();
  private revertErrorEvent = new Emitter<VaultError>();
  private downloadEvent = new Emitter<Download>();

  /** Everything is this many times faster. Tests pass a large number. */
  private readonly speed: number;
  private readonly manual: boolean;
  /**
   * One tick of whatever is running, or null when nothing is. In manual mode
   * the test calls it, so how far a run got is a decision the test makes
   * rather than a race with the machine it happens to be on.
   */
  private tick: (() => void) | null = null;

  /** The downloader: its services, its tokens and its queue. */
  downloads: DownloadDesk;

  constructor(
    options: { empty?: boolean; speed?: number; manual?: boolean } = {},
  ) {
    this.speed = options.speed ?? 1;
    this.manual = options.manual === true;
    this.downloads = this.newDesk();
    if (options.empty === true) {
      this.emptyWorld();
      // A person who has never opened this program has no vault either, and
      // the engine refuses everything but get_app_state until they choose one.
      this.vaultOpen = false;
    }
    else this.recordScan("scan-1");
  }

  private newDesk(): DownloadDesk {
    return new DownloadDesk(
      () => this.world,
      this.manual,
      () => this.tickMs,
      this.opened,
      (record) => this.downloadEvent.emit(record),
    );
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
    this.lastCancelledScanAt = null;
  }

  /**
   * The engine's clock, as the order of events sees it. Operations run one at
   * a time and each takes real time, so on the real engine a later one never
   * carries the same millisecond as the one before it. Here two can land in
   * the same millisecond on a fast machine, which would make "which came
   * first" a race with the machine the suite runs on.
   */
  private lastStamp = 0;
  private stamp(): string {
    this.lastStamp = Math.max(Date.now(), this.lastStamp + 1);
    return new Date(this.lastStamp).toISOString();
  }

  private recordScan(scanId: string): ScanRecord {
    const result = { ...scanResultOf(this.world, scanId), finishedAt: this.stamp() };
    this.lastScan = result;
    this.lastCancelledScanAt = null;
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
      lastCancelledScanAt: this.lastCancelledScanAt,
      lastPlanId: [...this.plans.keys()].at(-1) ?? null,
      interruptedApplies: this.applies
        .filter((a) => this.waitsToBeSettled(a))
        .map((a) => a.applyId),
      busy: this.busy,
    };
  }

  /**
   * Measured against the real engine: an empty folder, a folder not made yet
   * and an existing vault are accepted. A folder that holds files, or only
   * folders, is refused with conflict, and nothing in it changes.
   */
  async selectVault(path: string): Promise<VaultInfo> {
    const folder = this.disk.find((f) => f.path.toLowerCase() === path.toLowerCase());
    const isVault = path.toLowerCase() === VAULT_ROOT.toLowerCase();
    if (folder && !isVault && (folder.hasFiles === true || folder.children.length > 0)) {
      throw {
        code: "conflict",
        message:
          "That folder already holds files, so it cannot become the vault. Choose an empty folder, or make a new one.",
        path,
      } satisfies VaultError;
    }
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
      freeBytes: this.world.driveReadable ? this.world.freeBytes : null,
      totalBytes: this.world.driveReadable ? VAULT_TOTAL_BYTES : null,
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

  /**
   * Measured against the real engine: the search goes three folders down,
   * one level at a time with each level sorted, and never looks inside an
   * install it found. The first becomes the root and the rest are the other
   * candidates, whether or not any of them is registered already.
   */
  private findRoots(start: string): string[] {
    const found: string[] = [];
    let frontier = [start];
    for (let depth = 0; depth <= 3 && frontier.length > 0; depth += 1) {
      const next: string[] = [];
      for (const dir of [...frontier].sort()) {
        const folder = this.disk.find((f) => f.path === dir);
        if (!folder || folder.readable === false) continue;
        if (folder.children.some((c) => leafOf(c).toLowerCase() === "models")) {
          found.push(dir);
          continue;
        }
        next.push(...folder.children);
      }
      frontier = next;
    }
    return found;
  }

  async validateInstallPath(path: string): Promise<InstallCandidate> {
    const folder = this.disk.find((f) => f.path === path);
    if (!folder || folder.readable === false) {
      return invalidCandidate("ComfyVault could not read that folder.");
    }
    const roots = this.findRoots(path);
    const root = roots[0];
    if (!root) {
      return invalidCandidate("No models folder was found inside that folder.");
    }
    // An install this world already knows carries its real yaml, complaints
    // and all, so the picker shows what registering it will really read.
    const known = this.world.installs.find(
      (i) => i.root.toLowerCase() === root.toLowerCase(),
    );
    const extraPaths = known?.extraPaths ?? YAML_ON_DISK[root] ?? [];
    return {
      valid: true,
      root,
      nestedDepth: root.split("\\").filter(Boolean).length - path.split("\\").filter(Boolean).length,
      markersFound: [
        "main.py", "nodes.py", "folder_paths.py", "execution.py",
        "server.py", "comfy/", "comfy_extras/",
      ],
      markersMissing: [],
      contentCheckPassed: true,
      otherCandidates: roots.slice(1),
      version: known?.version ?? "0.29.1",
      versionSource: known?.versionSource ?? "comfyui_version.py",
      modelsDir: `${root}\\models`,
      modelsDirExists: true,
      extraPathsFile: extraPaths.length > 0 ? `${root}\\extra_model_paths.yaml` : null,
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
    if (!candidate.valid || !candidate.root) {
      throw error("notAComfyInstall", candidate.reason ?? "Not a ComfyUI install.");
    }
    const root = candidate.root;
    if (this.world.installs.some((i) => i.root.toLowerCase() === root.toLowerCase())) {
      throw error("alreadyRegistered", "That install is already in the list.");
    }
    const install: Install = {
      id: leafOf(root).toLowerCase().replace(/[^a-z0-9]+/g, "-"),
      label:
        label ??
        uniqueDefaultLabel(
          path,
          root,
          this.world.installs.map((i) => i.label),
        ),
      registeredPath: path,
      root,
      modelsDir: `${root}\\models`,
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
    // Measured against the real engine: forgetting an install never makes a
    // scan record. Before the first scan there is still none afterwards.
    if (this.lastScan) this.recordScan(this.lastScan.scanId);
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

  /**
   * As the real engine does: a cancelled scan is recorded, marked cancelled,
   * with every total at zero. It becomes the last scan only when no scan has
   * finished, and it leaves each install's last scan time and totals alone.
   */
  async cancelScan(scanId: string): Promise<{ cancelled: true }> {
    this.stopScanTimer();
    this.busy = null;
    const full = scanResultOf(this.world, scanId, true);
    const zero = <T extends object>(t: T): T =>
      Object.fromEntries(
        Object.entries(t).map(([k, v]) => [k, typeof v === "number" ? 0 : v]),
      ) as T;
    const record: ScanRecord = {
      ...full,
      finishedAt: this.stamp(),
      totals: zero(full.totals),
      perInstall: full.perInstall.map(zero),
    };
    if (this.lastScan === null || this.lastScan.cancelled) this.lastScan = record;
    this.lastCancelledScanAt = record.finishedAt;
    this.scanDoneEvent.emit(record);
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
    this.deletedSinceApply = [];
    // Read from the drive before anything moves, the way the engine reads it.
    const freeBefore = this.world.freeBytes;

    const bytesToMove = groups.filter((g) => !g.alreadyInVault).reduce((s, g) => s + g.sizeBytes, 0);
    const started = Date.now();
    // One file is written to between the report and the run, as really happens.
    const changesAt = groups.length > 7 ? 6 : -1;
    let reached = 0;
    let elapsed = 0;

    const step = () => {
      // The process ends where the run is: nothing more is committed, and the
      // record is left as it was written when the run started.
      if (this.cuttingOff) {
        this.cuttingOff = false;
        this.stopApplyTimer();
        this.busy = null;
        const done = groups.slice(0, reached).filter((_, i) => i !== changesAt);
        this.cutOffRuns.set(applyId, { done, pending: groups.slice(reached), freeBefore });
        this.applies = [
          {
            applyId,
            planId: args.planId,
            groupIds: args.groupIds,
            state: "running",
            startedAt: new Date(started).toISOString(),
            finishedAt: null,
            groupsRequested: groups.length,
            groupsApplied: done.length,
            groupsFailed: 0,
            bytesFreed: done.reduce((sum, g) => sum + g.bytesFreed, 0),
            vaultFreeBytesBefore: freeBefore,
            vaultFreeBytesAfter: null,
            filesMoved: done.filter((g) => !g.alreadyInVault).length,
            linksCreated: done.reduce((sum, g) => sum + g.occurrences, 0),
            failures: [],
            revertible: true,
            lastUndoStepAt: null,
          },
          ...this.applies,
        ];
        this.appliedGroups.set(applyId, done);
        return;
      }
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
        bytesMoved: done.filter((g) => !g.alreadyInVault).reduce((s, g) => s + g.sizeBytes, 0),
        bytesToMove,
        bytesFreed: done.reduce((s, g) => s + g.bytesFreed, 0),
        filesMoved: done.filter((g) => !g.alreadyInVault).length,
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
          filesMoved: done.filter((g) => !g.alreadyInVault).length,
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
          lastUndoStepAt: null,
        };
        this.applies = [result, ...this.applies];
        this.appliedGroups.set(applyId, done);
        // Measured against the real engine: a run does not record a scan. The
        // last scan still describes the world as it was before the run, and
        // anything built from it afterwards describes that older world.
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
    // Measured against the real engine: a run cut off comes back as state
    // "running", and that is what it lists here.
    return this.applies
      .filter((a) => this.waitsToBeSettled(a))
      .map((a) => ({
        applyId: a.applyId,
        planId: a.planId,
        startedAt: a.startedAt,
        stepsDone: a.groupsApplied,
        stepsPending: a.groupsRequested - a.groupsApplied,
        // The engine states only what its journal records.
        description: `An earlier run stopped before it finished. It completed ${a.groupsApplied} steps and left ${a.groupsRequested - a.groupsApplied} unfinished.`,
        affectedPaths: [],
        blocked: this.blockedRuns.has(a.applyId),
        blockedPaths: this.blockedRuns.get(a.applyId) ?? [],
      }));
  }

  async resumeApply(applyId: string): Promise<{ applyId: string }> {
    this.requireVault();
    if (this.busy) throw error("vaultBusy", "Something is already running.");
    this.refuseBlocked(applyId);
    const cut = this.cutOffRuns.get(applyId);
    if (!cut) throw error("notFound", "That run is not waiting to be finished.");
    this.busy = { kind: "apply", id: applyId };
    let reached = 0;
    const step = () => {
      const group = cut.pending[reached];
      if (group) {
        this.commitGroup(group);
        reached += 1;
      }
      const all = [...cut.done, ...cut.pending.slice(0, reached)];
      const finished = reached >= cut.pending.length;
      this.applyProgressEvent.emit({
        applyId,
        phase: finished ? "finalizing" : "applying",
        groupIndex: reached,
        groupTotal: cut.pending.length,
        currentGroupId: cut.pending[reached]?.groupId ?? null,
        currentPath: cut.pending[reached]?.source.absPath ?? null,
        step: "moving",
        bytesMoved: 0,
        bytesToMove: 0,
        bytesFreed: all.reduce((sum, g) => sum + g.bytesFreed, 0),
        filesMoved: all.filter((g) => !g.alreadyInVault).length,
        linksCreated: all.reduce((sum, g) => sum + g.occurrences, 0),
        failures: 0,
        elapsedMs: reached * TICK_MS,
        etaMs: null,
      });
      if (!finished) return;
      this.stopApplyTimer();
      this.busy = null;
      this.cutOffRuns.delete(applyId);
      this.appliedGroups.set(applyId, all);
      // The contract: a resumed run continues the same journal as one run, so
      // its record describes the whole run.
      this.applies = this.applies.map((a) =>
        a.applyId === applyId
          ? {
              ...a,
              state: "completed" as const,
              finishedAt: this.stamp(),
              groupsApplied: all.length,
              filesMoved: all.filter((g) => !g.alreadyInVault).length,
              linksCreated: all.reduce((sum, g) => sum + g.occurrences, 0),
              bytesFreed: all.reduce((sum, g) => sum + g.bytesFreed, 0),
              vaultFreeBytesAfter: this.world.freeBytes,
            }
          : a,
      );
      const record = this.applies.find((a) => a.applyId === applyId);
      if (record) this.applyDoneEvent.emit(record);
    };
    this.tick = step;
    if (!this.manual) this.applyTimer = setInterval(step, this.tickMs);
    return { applyId };
  }


  /** Why an undo would refuse before it starts, or null when it would not. */
  private revertRefusal(applyId: string): VaultError | null {
    if (!this.worldBeforeApply) return error("conflict", "There is nothing to put back.");
    // A delete cannot be undone, so the person is told why, not sent to undo it.
    if (this.deletedSinceApply.length > 0) {
      return error(
        "conflict",
        "One of this run's models was deleted in Cleanup, so this run can no longer be undone. Nothing was changed.",
        [...new Set(this.deletedSinceApply)].sort().join(", "),
      );
    }
    const before = this.worldBeforeApply;
    const stopped = [...this.stoppedDeletes]
      .filter(([sha]) => !before.vault.has(sha))
      .flatMap(([, paths]) => paths);
    if (stopped.length > 0) {
      return error(
        "conflict",
        "A delete of one of this run's models stopped part way, so this run was not undone. Nothing was changed. Finish the delete in Cleanup. After that, this run can no longer be undone.",
        [...new Set(stopped)].sort().join(", "),
      );
    }
    // Checked on the disk before any step: every model the run put in the vault.
    const missing = (this.appliedGroups.get(applyId) ?? []).find(
      (g) => !before.vault.has(g.sha256) && !this.world.vault.has(g.sha256),
    );
    if (missing) {
      const path = `${VAULT_ROOT}\\${missing.vaultRelPath}`;
      return {
        ...error(
          "conflict",
          "A model this run put in the vault is no longer there, so this run can no longer be undone. Nothing was changed.",
          path,
        ),
        path,
      };
    }
    if (this.renamedSinceApply.size > 0) {
      const paths = vaultFilesOf(this.world)
        .filter((f) => this.renamedSinceApply.has(f.sha256))
        .map((f) => `${VAULT_ROOT}\\${f.vaultRelPath}`);
      return error(
        "conflict",
        "Files this run created have been renamed since, so putting them back would lose the new names.",
        paths.join("\n"),
      );
    }
    return null;
  }

  /**
   * An undo's steps, newest first, the way the engine walks its journal: for
   * each group the links come out and the removed copies are copied back out
   * of the vault, and the file that moved into the vault is renamed back last.
   * A kept copy from another drive than the vault's cannot be renamed back
   * across drives, so it is copied back too, as the contract says.
   */
  private revertSteps(
    applyId: string,
  ): Array<{ action: RevertProgress["action"]; path: string; bytes: number }> {
    const steps: Array<{ action: RevertProgress["action"]; path: string; bytes: number }> = [];
    for (const g of this.appliedGroups.get(applyId) ?? []) {
      steps.push(
        volumeOfPath(g.source.absPath) === VAULT_VOLUME
          ? { action: "renamingBack", path: g.source.absPath, bytes: 0 }
          : { action: "copyingBack", path: g.source.absPath, bytes: g.sizeBytes },
      );
      for (const link of g.links) {
        steps.push({ action: "removingLink", path: link.absPath, bytes: 0 });
      }
      const copies = g.links.filter((l) => l.absPath !== g.source.absPath);
      for (const copy of copies.slice(0, Math.max(0, g.distinctFiles - 1))) {
        steps.push({ action: "copyingBack", path: copy.absPath, bytes: g.sizeBytes });
      }
    }
    return steps.reverse();
  }

  async previewRevert(applyId: string): Promise<RevertPreview> {
    this.requireVault();
    this.refuseBlocked(applyId);
    const refusal = this.revertRefusal(applyId);
    if (refusal) throw refusal;
    const all = this.revertSteps(applyId);
    const done = this.revertStepsDone.get(applyId) ?? 0;
    // What a stopped undo left to do. The files it already put back are read
    // from the disk by the engine, so they survive a restart.
    const steps = all.slice(done);
    // Each copy takes room on the drive it goes back to.
    const perDrive = new Map<string, number>();
    for (const st of steps) {
      if (st.bytes === 0) continue;
      const volume = volumeOfPath(st.path);
      perDrive.set(volume, (perDrive.get(volume) ?? 0) + st.bytes);
    }
    const drives = await this.listDrives();
    return {
      applyId,
      filesAlreadyBack: all.slice(0, done).filter((st) => st.action !== "removingLink").length,
      filesRenamedBack: steps.filter((st) => st.action === "renamingBack").length,
      filesCopiedBack: steps.filter((st) => st.action === "copyingBack").length,
      bytesToCopy: steps.reduce((sum, st) => sum + st.bytes, 0),
      drives: [...perDrive].map(([volume, bytes]) => ({
        volume: `${volume}\\`,
        predictedRoomBytes: this.revertRoomBytes ?? bytes,
        freeBytes:
          volume === VAULT_VOLUME
            ? this.world.driveReadable
              ? this.world.freeBytes
              : null
            : (drives.find((d) => d.root === `${volume}\\`)?.freeBytes ?? null),
      })),
    };
  }

  async revertApply(applyId: string): Promise<{ applyId: string }> {
    this.requireVault();
    if (this.busy) throw error("vaultBusy", "Something is already running.");
    this.refuseBlocked(applyId);
    const refusal = this.revertRefusal(applyId);
    if (refusal) throw refusal;
    const before = this.worldBeforeApply!;
    this.busy = { kind: "revert", id: applyId };
    this.applyCancelling = false;
    // The engine writes this as the undo starts, so a stop, a failure or a
    // closed app leaves the run saying it is part undone.
    this.applies = this.applies.map((a) =>
      a.applyId === applyId ? { ...a, state: "partlyReverted" as const } : a,
    );

    // Only the copies take time, so time here follows their bytes. A stopped
    // undo carries on from where it stopped.
    type Step = ReturnType<FixtureEngine["revertSteps"]>[number];
    const steps = this.revertSteps(applyId).slice(this.revertStepsDone.get(applyId) ?? 0);
    const bytesToCopy = steps.reduce((sum, st) => sum + st.bytes, 0);
    const filesToPutBack = steps.filter((st) => st.action !== "removingLink").length;
    const ticks = REVERT_MS / TICK_MS;
    const perTick = bytesToCopy > 0 ? bytesToCopy / ticks : 0;
    let index = 0;
    let intoStep = 0;
    let bytesCopied = 0;
    let filesPutBack = 0;
    let linksRemoved = 0;
    let elapsed = 0;

    const complete = (st: Step) => {
      // Written before each step, the way the engine writes it.
      const at = this.stamp();
      this.applies = this.applies.map((a) =>
        a.applyId === applyId ? { ...a, lastUndoStepAt: at } : a,
      );
      index += 1;
      this.revertStepsDone.set(applyId, (this.revertStepsDone.get(applyId) ?? 0) + 1);
      intoStep = 0;
      if (st.action === "removingLink") linksRemoved += 1;
      else filesPutBack += 1;
    };

    const step = () => {
      // A stop leaves the file it is part way through as it was, behind its
      // link, and everything already put back stays back.
      if (this.applyCancelling) {
        this.stopApplyTimer();
        this.busy = null;
        this.revertErrorEvent.emit(error("cancelled", "The operation was cancelled."));
        return;
      }
      elapsed += TICK_MS;
      let budget = perTick > 0 ? perTick : Infinity;
      let instant = perTick > 0 ? Infinity : Math.ceil(steps.length / ticks);
      while (index < steps.length) {
        const st = steps[index]!;
        if (st.bytes === 0) {
          if (instant <= 0) break;
          instant -= 1;
          complete(st);
          continue;
        }
        const take = Math.min(budget, st.bytes - intoStep);
        intoStep += take;
        bytesCopied += take;
        budget -= take;
        if (intoStep < st.bytes) break;
        complete(st);
      }
      const overall = index >= steps.length ? 1 : 0;
      const current = steps[index];
      this.revertProgressEvent.emit({
        applyId,
        phase: overall >= 1 ? "finalizing" : "restoring",
        stepIndex: index,
        stepTotal: steps.length,
        currentPath: current?.path ?? null,
        action: current?.action ?? "tidying",
        filesPutBack,
        filesToPutBack,
        linksRemoved,
        bytesCopied: Math.round(bytesCopied),
        bytesToCopy,
        elapsedMs: elapsed,
        etaMs: overall >= 1 ? null : Math.max(0, REVERT_MS - elapsed),
      });

      if (overall >= 1) {
        this.stopApplyTimer();
        this.busy = null;
        this.world = before;
        this.worldBeforeApply = null;
        this.renamedSinceApply.clear();
    this.deletedSinceApply = [];
        // Measured against the real engine: an undo stamps the record's
        // finishedAt again, so on a reverted run it is the time of the undo.
        // It does not record a scan.
        const finishedAt = this.stamp();
        this.applies = this.applies.map((a) =>
          a.applyId === applyId
            ? { ...a, state: "reverted" as const, revertible: false, finishedAt }
            : a,
        );
        const undone = this.applies.find((a) => a.applyId === applyId);
        if (undone) this.revertDoneEvent.emit(undone);
      }
    };
    this.tick = step;
    if (!this.manual) this.applyTimer = setInterval(step, this.tickMs);

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
  onRevertProgress(fn: (p: RevertProgress) => void): Unsubscribe {
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
    relativeDir?: string;
    dir?: string;
    linkName?: string;
  }): Promise<LinkRecord> {
    this.refuseLinksWhileRunning();
    const install = this.world.installs.find((i) => i.id === args.installId);
    if (!install) throw error("notFound", "That install is not registered.");
    const entry = this.world.vault.get(args.sha256);
    if (!entry) throw error("notFound", "The vault does not hold that file.");
    if ((args.dir === undefined) === (args.relativeDir === undefined)) {
      throw error("invalidArgument", "Give the folder as relativeDir or as dir, not both.");
    }
    const linkName = args.linkName ?? entry.canonicalName;
    let folder = `${install.root}\\${args.relativeDir}`;
    if (args.dir !== undefined) {
      // A full folder must be one ComfyUI reads for the model's kind, and is remembered.
      const category = this.world.contents.find((c) => c.sha256 === args.sha256)?.category ?? "";
      this.downloads.checkInside(install, category, args.dir);
      this.downloads.remember(install.id, category, args.dir);
      folder = args.dir;
    }
    const absPath = `${folder}\\${linkName}`;
    const there = this.world.links.find((l) => sameName(l.absPath, absPath));
    if (there || this.takenPaths.has(absPath.toLowerCase())) {
      // The engine's own words, used for nothing else.
      throw error(
        "conflict",
        there?.sha256 === args.sha256
          ? "This model is already linked here with that name. Nothing was changed."
          : "A different file with this name is already here. Choose another folder.",
      );
    }
    const link: LinkRecord = {
      id: `link-${this.world.links.length + 1}`,
      installId: args.installId,
      absPath,
      relPath: absPath.toLowerCase().startsWith(`${install.root}\\`.toLowerCase())
        ? absPath.slice(install.root.length + 1)
        : absPath,
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
    this.refuseLinksWhileRunning();
    if (this.lockedLinks.has(linkId)) {
      throw error("fileLocked", "Windows would not let the link go, because a program has it open.");
    }
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
    const all = vaultFilesOf(this.world, this.metadataCache);
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
    let rows = contentRowsOf(this.world, this.metadataCache);
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

  async planUnifyName(sha256: string, name: string): Promise<UnifyPlan> {
    this.requireVault();
    const links = this.unifySteps(sha256, name);
    const affected = [...new Set(links.map((s) => s.installId))];
    const goingAway = [...new Set(links.map((s) => s.linkName).filter((n) => !sameName(n, name)))];
    return {
      sha256,
      name,
      links,
      running: affected.filter((id) => this.world.running.includes(id)),
      workflows: await this.checkModelUsage(goingAway),
    };
  }

  async unifyName(sha256: string, name: string): Promise<UnifyResult> {
    this.requireVault();
    if (this.busy) throw error("vaultBusy", "Something is already running.");
    const steps = this.unifySteps(sha256, name);
    const running = [...new Set(steps.map((s) => s.installId))].filter((id) =>
      this.world.running.includes(id),
    );
    if (running.length > 0) {
      throw error(
        "conflict",
        `Close ${running.map((id) => this.world.installs.find((i) => i.id === id)?.label ?? id).join(" and ")} first`,
      );
    }
    const result: UnifyResult = {
      unifyId: `unify-${++this.unifyCount}`,
      name,
      vaultName: name,
      renamed: [],
      removed: [],
      skipped: [],
      stopped: null,
    };
    for (const step of steps) {
      const link = this.world.links.find((l) => l.absPath === step.absPath)!;
      if (step.action === "rename") {
        link.absPath = step.newAbsPath!;
        link.relPath = `${link.relPath.slice(0, link.relPath.lastIndexOf("\\") + 1)}${name}`;
        link.linkName = name;
        result.renamed.push({ installId: step.installId, from: step.absPath, to: step.newAbsPath! });
      } else if (step.action === "remove") {
        this.world.links = this.world.links.filter((l) => l !== link);
        result.removed.push({ installId: step.installId, path: step.absPath });
      } else if (step.action === "blockedTaken") {
        result.skipped.push({
          installId: step.installId,
          path: step.absPath,
          reason: `${step.takenBy} already has that name.`,
        });
      }
    }
    const entry = this.world.vault.get(sha256)!;
    if (entry.canonicalName !== name) {
      if (this.worldBeforeApply && !this.worldBeforeApply.vault.has(sha256)) {
        // This run put the file in the vault and it has been renamed since.
        this.renamedSinceApply.add(sha256);
      }
      entry.canonicalName = name;
    }
    // No install link reaches the file through any other name now.
    entry.aliases = [];
    return result;
  }

  /** What giving the model this name does to each of its links, as the engine plans it. */
  private unifySteps(sha256: string, name: string): UnifyLink[] {
    if (!this.world.vault.has(sha256)) {
      throw error("notFound", "The vault does not hold that file.");
    }
    const links = this.world.links.filter((l) => l.sha256 === sha256);
    if (!links.some((l) => sameName(l.linkName, name))) {
      throw error("invalidArgument", "That name is not one the installs use for this model.");
    }
    return links.map((link) => {
      const base = { installId: link.installId, absPath: link.absPath, linkName: link.linkName };
      if (sameName(link.linkName, name)) {
        return { ...base, action: "keep" as const, newAbsPath: null, takenBy: null };
      }
      const newAbsPath = `${link.absPath.slice(0, link.absPath.lastIndexOf("\\") + 1)}${name}`;
      const there = this.world.links.find((l) => sameName(l.absPath, newAbsPath));
      if (there && there.sha256 === sha256) {
        return { ...base, action: "remove" as const, newAbsPath: null, takenBy: null };
      }
      if (there || this.takenPaths.has(newAbsPath.toLowerCase())) {
        return { ...base, action: "blockedTaken" as const, newAbsPath: null, takenBy: newAbsPath };
      }
      return { ...base, action: "rename" as const, newAbsPath, takenBy: null };
    });
  }

  async getHiddenNameCards(): Promise<HiddenNameCard[]> {
    return this.hiddenNameCards.map((c) => ({ ...c, names: [...c.names] }));
  }

  async setHiddenNameCards(cards: HiddenNameCard[]): Promise<HiddenNameCard[]> {
    this.hiddenNameCards = cards.map((c) => ({ sha256: c.sha256, names: [...new Set(c.names)].sort() }));
    return this.getHiddenNameCards();
  }

  async listOrphans(): Promise<VaultFile[]> {
    this.requireVault();
    return vaultFilesOf(this.world).filter((f) => f.linkCount === 0);
  }

  async deleteVaultFile(
    sha256: string,
    confirm: string,
    removeLinks = false,
  ): Promise<Deleted> {
    if (removeLinks) return this.deleteModelAndLinks(sha256, confirm);
    this.refuseLinksWhileRunning();
    if (confirm !== sha256) {
      throw error("invalidArgument", "This delete was not confirmed, so nothing was removed.");
    }
    if (!this.world.vault.has(sha256)) {
      throw error("notFound", "That model is not in the vault.");
    }
    const linked = this.world.links.filter((l) => l.sha256 === sha256 && this.stateOf(l) === "ok");
    if (linked.length > 0) {
      throw error(
        "conflict",
        "Some installs still link to that model, so it was kept. Remove those links first.",
        linked.map((l) => l.absPath).join(", "),
      );
    }
    const content = this.world.contents.find((c) => c.sha256 === sha256);
    this.world.vault.delete(sha256);
    const bytesFreed = content?.bytes ?? 0;
    this.world.freeBytes += bytesFreed;
    return { deleted: true, bytesFreed, linksRemoved: [] };
  }

  /**
   * The model and every link to it, as section 8.8 of the contract says: every
   * path is checked before anything is removed, and a refusal removes nothing.
   */
  private deleteModelAndLinks(sha256: string, confirm: string): Deleted {
    if (this.busy) throw error("vaultBusy", "Something is already running.");
    if (confirm !== sha256) {
      throw error("invalidArgument", "This delete was not confirmed, so nothing was removed.");
    }
    const entry = this.world.vault.get(sha256);
    if (!entry) throw error("notFound", "That model is not in the vault.");
    const content = this.world.contents.find((c) => c.sha256 === sha256);
    const links = this.world.links.filter((l) => l.sha256 === sha256);

    // A real file where a link was is somebody's file, and it stops the delete.
    const surprises = links
      .filter((l) => this.stateOf(l) === "replaced")
      .map((l) => l.absPath)
      .sort();
    if (surprises.length > 0) {
      throw {
        ...error(
          "conflict",
          "Some of this model's links are not links to it any more, so nothing was deleted. Something else sits at these paths now.",
          surprises.join(", "),
        ),
        path: surprises[0],
      } satisfies VaultError;
    }

    const vaultPath = `${VAULT_ROOT}\\${content?.category ?? ""}\\${entry.canonicalName}`;
    if (this.heldOpen.has(sha256)) {
      throw {
        ...error(
          "fileLocked",
          "Another program has this model open, so nothing was deleted. ComfyUI keeps a model open while it is loaded. Close it and try again.",
          vaultPath,
        ),
        path: vaultPath,
      } satisfies VaultError;
    }

    // A link already gone from the disk is not counted as removed.
    const linksRemoved = links.filter((l) => this.stateOf(l) === "ok").map((l) => l.absPath);
    const removed = new Set(links.map((l) => l.absPath));
    if (this.worldBeforeApply && !this.worldBeforeApply.vault.has(sha256)) {
      // The run put this model in the vault, and its undo needs the file.
      this.deletedSinceApply.push(vaultPath, ...(this.stoppedDeletes.get(sha256) ?? []), ...linksRemoved);
    }
    this.stoppedDeletes.delete(sha256);
    if (content) content.copies = content.copies.filter((c) => !(c.isLink && removed.has(c.absPath)));
    this.world.links = this.world.links.filter((l) => l.sha256 !== sha256);
    this.world.vault.delete(sha256);
    const bytesFreed = content?.bytes ?? 0;
    this.world.freeBytes += bytesFreed;
    return { deleted: true, bytesFreed, linksRemoved };
  }

  /** Vault files another program holds open, by their SHA-256. */
  private heldOpen = new Set<string>();

  /** A program, ComfyUI say, opens a vault file or lets it go. */
  devHoldOpen(sha256: string, open = true): void {
    if (open) this.heldOpen.add(sha256);
    else this.heldOpen.delete(sha256);
  }

  /**
   * A delete with links is cut off, by the power going say, after it removed
   * this many of the model's links. The rest and the vault file are still there.
   */
  devStopDelete(sha256: string, linksGone = 1): string[] {
    const live = this.world.links.filter((l) => l.sha256 === sha256 && this.stateOf(l) === "ok");
    const gone = live.slice(0, linksGone).map((l) => l.absPath);
    const content = this.world.contents.find((c) => c.sha256 === sha256);
    if (content) content.copies = content.copies.filter((c) => !gone.includes(c.absPath));
    this.stoppedDeletes.set(sha256, [...(this.stoppedDeletes.get(sha256) ?? []), ...gone]);
    return gone;
  }

  /** Someone puts a real file where a link was, at this path. */
  devReplaceLink(absPath: string): void {
    for (const content of this.world.contents) {
      for (const copy of content.copies) {
        if (copy.absPath === absPath) copy.isLink = false;
      }
    }
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
    const stoppedDeletes = vaultFilesOf(this.world).filter((f) => this.stoppedDeletes.has(f.sha256));
    return {
      checkedLinks: this.world.links.length,
      checkedFiles: this.world.vault.size,
      danglingLinks,
      replacedLinks,
      missingVaultFiles,
      foreignFiles: [],
      stoppedDeletes,
      ok:
        danglingLinks.length === 0 &&
        replacedLinks.length === 0 &&
        missingVaultFiles.length === 0 &&
        stoppedDeletes.length === 0,
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
          installId: "studio",
          installLabel: "ComfyUI-Studio",
          workflowPath: `C:\\ComfyUI-Studio\\user\\default\\workflows\\flow-${i + 1}.json`,
          workflowName: `flow-${i + 1}`,
        })),
        method: USAGE_METHOD,
      };
    });
  }

  async getMetadata(sha256: string, refresh = false): Promise<ModelMetadata | null> {
    this.requireVault();
    const [answer] = await this.fetchMetadataBatch([sha256], refresh);
    return this.metadataCache.get(sha256) ?? (answer?.found ? answer : null);
  }

  /**
   * Measured against the real engine and the live service: a known hash comes
   * back found and is cached, an unknown one comes back found: false and is
   * cached too. With lookups off nothing goes out: every hash with no cached
   * answer comes back found: false and nothing is cached. Offline, a call with
   * `refresh` rejects with networkUnavailable.
   */
  async fetchMetadataBatch(hashes: string[], refresh = false): Promise<ModelMetadata[]> {
    this.requireVault();
    const out: ModelMetadata[] = [];
    const toFetch: string[] = [];
    for (const sha of hashes) {
      const cached = this.metadataCache.get(sha);
      if (cached && !refresh) out.push(cached);
      else toFetch.push(sha);
    }
    if (toFetch.length === 0 || !this.world.metadataLookupsEnabled) {
      return [...out, ...toFetch.map(notFoundMetadata)];
    }
    if (!this.civitaiReachable) {
      if (refresh) {
        throw error(
          "networkUnavailable",
          "Could not reach Civitai, so model details are not available right now. Everything else still works.",
        );
      }
      return [...out, ...toFetch.map(notFoundMetadata)];
    }
    // One request for every hundred hashes, as the engine sends them.
    this.civitaiRequests += Math.ceil(toFetch.length / 100);
    for (const sha of toFetch) {
      const known = this.world.contents.find((c) => c.sha256 === sha)?.civitai ?? null;
      const answer = known ? { ...known, fetchedAt: this.stamp() } : notFoundMetadata(sha);
      this.metadataCache.set(sha, answer);
      out.push(answer);
    }
    return out;
  }

  /**
   * Measured against the real engine: a cut-off run that names places outside
   * the vault and the installs is refused by finish, undo and the undo's cost
   * check with pathOutsideBoundary. Setting it aside changes only the record:
   * state setAside, not revertible, finishedAt still null, and it leaves the
   * list of runs to settle. A second set-aside is refused with conflict.
   */
  async setAsideRun(applyId: string): Promise<ApplyRecord> {
    this.requireVault();
    const run = this.applies.find((a) => a.applyId === applyId);
    if (!run || run.state !== "running" || this.busy?.id === applyId) {
      throw error("conflict", "Only a run that was cut off part way can be set aside.");
    }
    if (!this.blockedRuns.has(applyId)) {
      throw error("conflict", "This run can be finished or undone, so it is not set aside.");
    }
    const record = { ...run, state: "setAside" as const, revertible: false };
    this.applies = this.applies.map((a) => (a.applyId === applyId ? record : a));
    return record;
  }

  /**
   * A run cut off with nothing running it, or a run set aside whose places can
   * be reached again, which comes back to be finished or undone.
   */
  private waitsToBeSettled(a: ApplyRecord): boolean {
    if (this.busy?.id === a.applyId) return false;
    if (a.state === "running") return true;
    return a.state === "setAside" && !this.blockedRuns.has(a.applyId);
  }

  private refuseBlocked(applyId: string): void {
    const paths = this.blockedRuns.get(applyId);
    if (paths) {
      throw error(
        "pathOutsideBoundary",
        "This run names files outside the vault and the registered installs, so nothing was touched.",
        paths.join("\n"),
      );
    }
  }

  /** The places a cut-off or set-aside run names can be reached again. */
  devUnblockRun(applyId: string): void {
    this.blockedRuns.delete(applyId);
  }

  /** The cut-off run names these places, which are no longer in any install. */
  devBlockCutOffRun(paths: string[]): void {
    const run = this.applies.find((a) => a.state === "running");
    if (!run) throw new Error("devBlockCutOffRun: no run was cut off");
    this.blockedRuns.set(run.applyId, paths);
  }

  /** How many requests have gone out to Civitai. */
  devCivitaiRequests(): number {
    return this.civitaiRequests;
  }

  /** Civitai answers, or the network does not reach it. */
  devSetCivitaiReachable(reachable: boolean): void {
    this.civitaiReachable = reachable;
  }

  // ── running programs ──────────────────────────────────────────────────────

  async getRunningComfy(): Promise<RunningComfy[]> {
    this.requireVault();
    // Measured against the real engine: with no scan and an empty vault there
    // is no model file to ask about, so whether one is held is not known.
    const nothingToAsk = this.lastScan === null && this.world.vault.size === 0;
    return this.world.running.map((installId, i) => {
      const root = this.world.installs.find((x) => x.id === installId)?.root ?? null;
      const facts = this.processFacts;
      return {
        pid: 18244 + i,
        name: "python.exe",
        exePath: root ? `${root}\\python_embeded\\python.exe` : null,
        cwd: root,
        commandLine: facts.commandLine,
        matchedInstallIds: [installId],
        matchReason: "exeUnderRoot" as const,
        startedAt: facts.startedAt,
        listeningPorts: facts.listeningPorts,
        holdsModelFiles: nothingToAsk ? null : facts.holdsModelFiles,
      };
    });
  }

  /**
   * What Windows says about each running ComfyUI. By default one started the
   * evening before, still serving on 8188. A test sets each answer, including
   * none at all.
   */
  private processFacts: ProcessFacts = {
    startedAt: yesterdayAt(18, 42),
    listeningPorts: [8188],
    holdsModelFiles: true,
    commandLine: ["python.exe", "main.py", "--port", "8188"],
  };

  devSetProcessFacts(facts: Partial<ProcessFacts>): void {
    this.processFacts = { ...this.processFacts, ...facts };
  }

  async checkLockedFiles(paths: string[]): Promise<LockState[]> {
    return paths.map((path) => ({
      path,
      locked: LOCKED_PATHS.has(path) && this.world.running.length > 0,
      checkable: true,
      detail: null,
    }));
  }

  // ── downloads ─────────────────────────────────────────────────────────────

  // The token commands answer before a vault is open.
  setToken(service: TokenService, token: string) {
    return this.downloads.setToken(service, token);
  }
  getTokenStatus(service: TokenService) {
    return this.downloads.getTokenStatus(service);
  }
  removeToken(service: TokenService) {
    return this.downloads.removeToken(service);
  }
  readModelAddress(args: Parameters<DownloadDesk["readModelAddress"]>[0]) {
    this.requireVault();
    return this.downloads.readModelAddress(args);
  }
  openHuggingFacePage(owner: string, repo: string) {
    return this.downloads.openHuggingFacePage(owner, repo);
  }
  startDownload(args: Parameters<DownloadDesk["startDownload"]>[0]) {
    this.requireVault();
    return this.downloads.startDownload(args);
  }
  stopDownload(downloadId: string) {
    return this.downloads.stopDownload(downloadId);
  }
  continueDownload(downloadId: string) {
    return this.downloads.continueDownload(downloadId);
  }
  discardDownload(downloadId: string) {
    return this.downloads.discardDownload(downloadId);
  }
  listLinkFolders(args: { installId: string; category: string; dir?: string }) {
    this.requireVault();
    return this.downloads.listLinkFolders(args);
  }
  removeDownload(downloadId: string) {
    return this.downloads.removeDownload(downloadId);
  }
  async listDownloads() {
    this.requireVault();
    return this.downloads.listDownloads();
  }
  onDownloadProgress(fn: (record: Download) => void): Unsubscribe {
    return this.downloadEvent.on(fn);
  }

  // ── the window ────────────────────────────────────────────────────────────

  /** Every address the app asked the system to open, in order. */
  opened: string[] = [];

  async openExternal(target: string): Promise<void> {
    this.opened.push(target);
  }

  /** Whether Windows opens a Civitai page when asked. */
  private civitaiPageOpens = true;
  /** Every Civitai page the app asked for, by its numbers, in order. */
  civitaiPagesOpened: { modelId: number; versionId: number | null }[] = [];

  devSetCivitaiPageOpens(opens: boolean): void {
    this.civitaiPageOpens = opens;
  }

  /**
   * As the engine does: the page is named by whole numbers only, anything else
   * is refused before anything opens, and it answers before a vault exists.
   */
  async openCivitaiPage(modelId: number, versionId: number | null): Promise<null> {
    if (!Number.isInteger(modelId) || (versionId !== null && !Number.isInteger(versionId))) {
      throw error("invalidArgument", "A Civitai page is named by whole numbers.");
    }
    if (!this.civitaiPageOpens) {
      throw error("ioError", "Windows did not open the Civitai page.");
    }
    this.civitaiPagesOpened.push({ modelId, versionId });
    return null;
  }

  /** Whether Windows starts Task Manager when asked. */
  private taskManagerStarts = true;
  /** How many times Task Manager was asked for, so a test can see it. */
  taskManagerOpened = 0;

  devSetTaskManagerStarts(starts: boolean): void {
    this.taskManagerStarts = starts;
  }

  /** It answers before a vault is chosen, and returns null, as the engine does. */
  async openTaskManager(): Promise<null> {
    if (!this.taskManagerStarts) {
      throw error(
        "ioError",
        "Windows did not open Task Manager. Press Ctrl+Shift+Esc to open it.",
      );
    }
    this.taskManagerOpened += 1;
    return null;
  }
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
    this.devSetRunningInstalls(running ? ["studio"] : []);
  }

  /** ComfyUI running out of each of these installs, one process each. */
  devSetRunningInstalls(installIds: string[]): void {
    const running = installIds.length > 0;
    this.world.running = [...installIds];
    for (const content of this.world.contents) {
      for (const copy of content.copies) {
        if (copy.blocked === "fileLocked" && !running) copy.blocked = null;
        else if (copy.blocked === null && running && LOCKED_PATHS.has(copy.absPath)) {
          copy.blocked = "fileLocked";
        }
      }
    }
    // Starting or closing ComfyUI never makes a scan record where none was.
    if (this.lastScan) this.recordScan(this.lastScan.scanId);
  }

  /**
   * Take a file out of the vault from underneath its links, the way something
   * outside ComfyVault would. Every link to it then points at nothing.
   */
  /**
   * As in the engine: a consolidation or an undo moves files and makes and
   * removes links, so link changes are refused until it ends.
   */
  private refuseLinksWhileRunning(): void {
    if (this.busy?.kind === "apply" || this.busy?.kind === "revert") {
      throw error("vaultBusy", "Something is already running.");
    }
  }

  /** Links Windows will not let go of, as when a program holds one open. */
  private lockedLinks = new Set<string>();

  devLockLink(linkId: string): void {
    this.lockedLinks.add(linkId);
  }

  /** Put some other file at a place in an install, so a name is taken there. */
  devTakePath(absPath: string): void {
    this.takenPaths.add(absPath.toLowerCase());
  }

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
   * The process is cut off in the middle of the running apply, the way a crash
   * or a closed app ends it: the run stays on record as `running`.
   */
  devCutOffApply(): void {
    if (this.busy?.kind !== "apply") throw new Error("devCutOffApply: no run is going");
    this.cuttingOff = true;
    this.tick?.();
  }

  /**
   * A model is downloaded again into an install, as a real file, after an
   * earlier run already put it in the vault.
   */
  devDownloadAgain(sha256: string, installId: string, folder: string): string {
    const content = this.world.contents.find((c) => c.sha256 === sha256);
    const install = this.world.installs.find((i) => i.id === installId);
    if (!content || !install) throw new Error("devDownloadAgain: no such model or install");
    const absPath = `${install.root}\\${folder}${content.filename}`;
    content.copies.push({
      installId,
      folder,
      name: content.filename,
      absPath,
      relPath: `${folder}${content.filename}`,
      volume: absPath.slice(0, 2).toUpperCase(),
      isLink: false,
      blocked: null,
      sharesBytes: false,
    });
    return absPath;
  }

  /** The copies an undo makes occupy this much, as sparse files would. */
  devSetRevertRoom(bytes: number | null): void {
    this.revertRoomBytes = bytes;
  }

  /** The running undo stops on an error, the way the engine reports one. */
  devFailRevert(message: string): void {
    if (this.busy?.kind !== "revert") throw new Error("devFailRevert: no undo is running");
    this.stopApplyTimer();
    this.busy = null;
    this.revertErrorEvent.emit(error("ioError", message));
  }

  /**
   * The vault's drive stops answering when asked how much room it has, which
   * is what a network drive or a pulled card reader does.
   */
  devSetDriveReadable(readable: boolean): void {
    this.world.driveReadable = readable;
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
    this.downloads.dispose();
    this.downloads = this.newDesk();
    if (empty) this.emptyWorld();
    else this.recordScan("scan-1");
  }
}

/** What Windows says about a running ComfyUI in this world. */
export interface ProcessFacts {
  startedAt: string | null;
  listeningPorts: number[] | null;
  holdsModelFiles: boolean | null;
  commandLine: string[];
}

/** An ISO time for the day before today, at this local hour and minute. */
function yesterdayAt(hour: number, minute: number): string {
  const d = new Date();
  return new Date(d.getFullYear(), d.getMonth(), d.getDate() - 1, hour, minute).toISOString();
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

/** The drive a path is on, for example "C:". */
function volumeOfPath(path: string): string {
  return path.slice(0, 2).toUpperCase();
}

function notFoundMetadata(sha256: string): ModelMetadata {
  return {
    sha256,
    source: "civitai",
    fetchedAt: new Date().toISOString(),
    found: false,
    modelName: null,
    modelType: null,
    versionName: null,
    baseModel: null,
    triggerWords: [],
    nsfw: false,
    nsfwLevel: 0,
    civitaiModelId: null,
    civitaiVersionId: null,
    pageUrl: null,
    downloadUrl: null,
    ambiguous: false,
  };
}

/**
 * The engine's name for a new install: the folder the person picked, or the
 * root when they picked the root, else the first folder above it that no other
 * install is called, else the whole root. A launcher keeps the real install in
 * a folder called ComfyUI, so the root's own name tells installs apart least.
 */
export function uniqueDefaultLabel(
  picked: string,
  root: string,
  taken: readonly string[],
): string {
  const trim = (p: string) => p.replace(/[\\/]+$/, "");
  const start = trim(picked).toLowerCase() === trim(root).toLowerCase() ? root : picked;
  const isTaken = (name: string) =>
    taken.some((t) => t.trim().toLowerCase() === name.toLowerCase());
  const folders = start.split(/[\\/]+/).filter((s) => s.length > 0 && !/^[A-Za-z]:$/.test(s));
  for (let i = folders.length - 1; i >= 0; i--) {
    if (!isTaken(folders[i]!)) return folders[i]!;
  }
  return root;
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

/** Windows compares file names without regard to case. */
function sameName(a: string, b: string): boolean {
  return a.toLowerCase() === b.toLowerCase();
}
