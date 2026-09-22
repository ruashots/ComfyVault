/**
 * The shapes the interface reads from the engine.
 *
 * These mirror docs/IPC-CONTRACT.md, which comfyvault-core owns. Nothing else in
 * the frontend talks to Tauri directly: every screen reads these types through
 * src/ipc/client.ts, so the fixture backend and the real engine are
 * interchangeable.
 *
 * Every size is in BYTES. Every path is a Windows path as the engine reports it.
 * Every timestamp is an ISO 8601 string.
 */

export type InstanceId = string;

/** The ComfyUI folder a weight belongs in. The engine reports the folder it found. */
export type ModelFolder = string;

// ── installs ────────────────────────────────────────────────────────────────

export interface Instance {
  id: InstanceId;
  /** Short name shown everywhere in the interface. */
  name: string;
  /** Absolute path of the install root. */
  path: string;
  /** A ComfyUI process is running out of this install right now. */
  running: boolean;
  /** Extra folders this install's extra_model_paths.yaml adds, or null when it has none. */
  extraModelPaths: string[] | null;
  /** When the person registered it. */
  addedAt: string;
}

/** An install the person removed. Its files can still be in the vault. */
export interface RemovedInstance {
  name: string;
  path: string;
  removedAt: string;
}

// ── models ──────────────────────────────────────────────────────────────────

/** Why one placement cannot be moved right now. */
export type BlockedReason =
  | { kind: "file_open"; process: string; pid: number; instanceId: InstanceId }
  | { kind: "other_drive"; drive: string; vaultDrive: string }
  | { kind: "permission_denied" };

export type BlockedKind = BlockedReason["kind"];

/** One place on disk where a model's bytes are reachable today. */
export interface Placement {
  /** Stable for the life of a scan. */
  id: string;
  instanceId: InstanceId;
  /**
   * The folder holding the file, as the plan shows it: relative to the install
   * root when it sits under it, absolute when extra_model_paths.yaml added it.
   * Ends with a path separator.
   */
  folder: string;
  /** The filename used at this place. It can differ from the model's filename. */
  filename: string;
  /** Absolute path of the file. */
  fullPath: string;
  /** True once this path holds a link into the vault instead of the file. */
  isLink: boolean;
  /** Null when this placement can move right now. */
  blocked: BlockedReason | null;
}

/** What a Civitai hash lookup returned. The engine sends nothing else. */
export interface CivitaiMatch {
  name: string;
  version: string;
  type: string;
  baseModel: string;
  uploader: string;
}

/** One unique file, by content. */
export interface Model {
  /** Stable id. The engine derives it from the hash. */
  id: string;
  sha256: string;
  /** The canonical filename. */
  filename: string;
  folder: ModelFolder;
  bytes: number;
  /** Every place these bytes are reachable. Empty when only the vault holds them. */
  placements: Placement[];
  /** How many saved workflow files name this file, under any of its names. */
  workflowHits: number;
  /**
   * How many workflow files name it under each name it answers to. Cleanup
   * reads this to suggest which name the vault keeps.
   */
  workflowHitsByName: Record<string, number>;
  civitai: CivitaiMatch | null;
  /** Set when the vault already holds this file, null when it does not yet. */
  inVaultSince: string | null;
}

/** Weights ComfyVault counts and never moves. */
export interface CountedNeverMoved {
  kind: "custom_nodes" | "huggingface_cache";
  bytes: number;
  where: string;
}

// ── machine ─────────────────────────────────────────────────────────────────

export interface ComfyProcess {
  instanceId: InstanceId;
  process: string;
  pid: number;
  openFiles: number;
}

export interface DriveInfo {
  /** "C:" */
  letter: string;
  totalBytes: number;
  freeBytes: number;
}

/** Everything the interface must know before it lets Apply run. */
export interface MachineState {
  /** Windows will not let this program create a link while this is off. */
  developerMode: boolean;
  running: ComfyProcess[];
  vaultPath: string;
  vaultDrive: DriveInfo;
}

// ── scan ────────────────────────────────────────────────────────────────────

export interface ActivityEntry {
  event: string;
  detail: string;
  when: string;
}

export interface ScanResult {
  scannedAt: string;
  instances: Instance[];
  models: Model[];
  countedNeverMoved: CountedNeverMoved[];
  removedInstance: RemovedInstance | null;
  activity: ActivityEntry[];
  /** True when Civitai lookup ran during this scan. */
  civitaiEnabled: boolean;
}

export type ScanStepId =
  | "read_folders"
  | "read_yaml"
  | "list_files"
  | "hash_files"
  | "civitai";

export interface ScanProgress {
  currentStep: ScanStepId;
  /** 0 to 1 across the whole scan. */
  overall: number;
  etaSeconds: number | null;
  /** Each field is null until its step produces it. */
  instancesRead: number | null;
  extraFolders: string[] | null;
  filesListed: number | null;
  bytesListed: number | null;
  filesHashed: number;
  filesToHash: number;
  bytesHashed: number;
  bytesToHash: number;
  civitaiMatched: number | null;
  /** The file being read right now. */
  current: { name: string; path: string } | null;
  /** Null until the engine has read enough files to tell two of them apart. */
  found: {
    models: number;
    duplicateCopies: number;
    reclaimableBytes: number;
  } | null;
}

// ── apply ───────────────────────────────────────────────────────────────────

/** One line the engine appended to moves.log. */
export interface MoveLogLine {
  at: string;
  verb: "move" | "link" | "skip";
  detail: string;
}

export type SkipReason =
  | { kind: "changed_since_report" }
  | { kind: "blocked"; blocked: BlockedReason };

export interface SkippedFile {
  modelId: string;
  filename: string;
  bytes: number;
  reason: SkipReason;
}

export interface ApplyProgress {
  overall: number;
  etaSeconds: number | null;
  filesMoved: number;
  filesTotal: number;
  linksCreated: number;
  linksTotal: number;
  bytesMoved: number;
  bytesTotal: number;
  current: {
    name: string;
    fromInstance: string;
    fromPath: string;
    toPath: string;
    linkInstance: string;
    linkPath: string;
  } | null;
  skipped: SkippedFile[];
  /** Everything written to moves.log so far, oldest first. */
  log: MoveLogLine[];
  /** The engine is finishing the file in progress and will then stop. */
  stopping: boolean;
}

export interface ApplyResult {
  runId: string;
  startedAt: string;
  finishedAt: string;
  filesMoved: number;
  linksCreated: number;
  bytesFreed: number;
  /** How many files the vault had to rename to keep same-named files apart. */
  renamedInVault: number;
  leftAlone: { files: number; bytes: number };
  skipped: SkippedFile[];
  log: MoveLogLine[];
  logPath: string;
  /** False once a revert has run, or when the log is gone. */
  revertable: boolean;
}

// ── folder picker ───────────────────────────────────────────────────────────

export interface FolderEntry {
  path: string;
  name: string;
  kind: "drive" | "folder";
  /** The engine found a models folder inside. Null when it did not look. */
  looksLikeInstall: boolean | null;
  /** False when the engine knows it cannot be opened. */
  readable: boolean;
}

export type PickerPurpose = "instance" | "vault" | "link";

export type InstanceCheck =
  | { for: "instance"; ok: false; reason: "no_models_folder" }
  | { for: "instance"; ok: false; reason: "already_registered"; instanceId: InstanceId }
  | { for: "instance"; ok: false; reason: "unreadable" }
  | {
      for: "instance";
      ok: true;
      modelFolders: number;
      files: number;
      bytes: number;
      hasExtraModelPaths: boolean;
      /** The install is on a different drive than the vault. */
      onDifferentDrive: boolean;
      /** This is an install the person removed earlier. */
      wasRemoved: boolean;
    };

export type VaultCheck =
  | { for: "vault"; ok: false; reason: "not_writable" }
  | { for: "vault"; ok: false; reason: "inside_an_install"; instanceId: InstanceId }
  | {
      for: "vault";
      ok: true;
      drive: string;
      freeBytes: number;
      /** Every registered install sits on the same drive as this folder. */
      sameDriveAsInstalls: boolean;
      /** Installs that do not, by name. */
      installsOnOtherDrives: string[];
    };

export type LinkCheck =
  | { for: "link"; ok: false; reason: "outside_every_install" }
  | { for: "link"; ok: false; reason: "not_writable" }
  | { for: "link"; ok: false; reason: "already_holds_this_name"; filename: string }
  | { for: "link"; ok: true; instanceId: InstanceId };

export type FolderCheck = InstanceCheck | VaultCheck | LinkCheck;

export type CreateFolderResult =
  | { ok: true; path: string }
  | { ok: false; reason: "exists" | "denied" | "invalid_name" };

// ── the client surface ──────────────────────────────────────────────────────

export type Unsubscribe = () => void;

/**
 * Everything the interface can ask of the engine. The fixture backend and the
 * Tauri backend both implement this, so no screen knows which one it has.
 */
export interface Engine {
  /** The last scan the engine holds, or null when nothing was ever scanned. */
  loadScan(): Promise<ScanResult | null>;
  readMachine(): Promise<MachineState>;

  startScan(): Promise<void>;
  cancelScan(): Promise<void>;
  onScanProgress(fn: (p: ScanProgress) => void): Unsubscribe;
  onScanFinished(fn: (r: ScanResult) => void): Unsubscribe;
  onScanCancelled(fn: () => void): Unsubscribe;

  /** modelIds are the models the person left ticked. */
  startApply(modelIds: string[]): Promise<void>;
  /** Finish the file in progress, then stop. */
  stopApply(): Promise<void>;
  onApplyProgress(fn: (p: ApplyProgress) => void): Unsubscribe;
  onApplyFinished(fn: (r: ApplyResult) => void): Unsubscribe;
  /** The last finished run, so the report survives a restart. */
  lastRun(): Promise<ApplyResult | null>;
  revert(runId: string): Promise<void>;

  listFolder(path: string | null, purpose: PickerPurpose): Promise<FolderEntry[]>;
  checkFolder(path: string, purpose: PickerPurpose): Promise<FolderCheck>;
  createFolder(parent: string, name: string): Promise<CreateFolderResult>;

  addInstance(path: string): Promise<ScanResult>;
  /** Point an install that is already registered at a different folder. */
  setInstancePath(id: InstanceId, path: string): Promise<ScanResult>;
  removeInstance(id: InstanceId): Promise<ScanResult>;
  setVaultPath(path: string): Promise<MachineState>;
  setCivitaiEnabled(on: boolean): Promise<void>;

  /** Settle a model on one name inside the vault. */
  setVaultName(modelId: string, name: string): Promise<ScanResult>;
  /** Stop offering one of a model's names. */
  dropName(modelId: string, name: string): Promise<ScanResult>;
  /** Put a link to a vault file inside an install. */
  addLink(modelId: string, folder: string): Promise<ScanResult>;
  /** Delete a vault file nothing points at. */
  deleteOrphan(modelId: string): Promise<ScanResult>;
  /**
   * Take one copy that sits on another drive into the plan anyway, as a copy
   * rather than a move. The engine decides what that costs and returns the plan
   * that follows from it.
   */
  markCopyInstead(modelId: string, placementId: string): Promise<ScanResult>;

  openWindowsDeveloperSettings(): Promise<void>;
  openInExplorer(path: string): Promise<void>;

  /** Titlebar. Tauri owns the real window. */
  windowMinimize(): Promise<void>;
  windowToggleMaximize(): Promise<void>;
  windowClose(): Promise<void>;
}
