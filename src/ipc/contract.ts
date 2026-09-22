/**
 * The shapes the engine speaks, copied from docs/IPC-CONTRACT.md version 1.
 *
 * comfyvault-core owns that document. Nothing here is invented: if the
 * interface needs a fact that is not below, it is asked for and the document
 * changes first.
 *
 * Every byte count is an integer number of bytes. Every timestamp is RFC 3339
 * in UTC. Every hash is 64 hexadecimal characters, SHA-256.
 */

// ── errors ──────────────────────────────────────────────────────────────────

export type ErrorCode =
  | "notInitialized"
  | "vaultBusy"
  | "invalidArgument"
  | "notFound"
  | "pathOutsideBoundary"
  | "notAComfyInstall"
  | "alreadyRegistered"
  | "ioError"
  | "permissionDenied"
  | "fileLocked"
  | "fileChanged"
  | "symlinkUnsupported"
  | "storeError"
  | "parseError"
  | "networkUnavailable"
  | "cancelled"
  | "conflict";

export interface VaultError {
  code: ErrorCode;
  /** One sentence, written for a person to read. */
  message: string;
  /** Technical text, for a details panel. */
  detail?: string;
  path?: string;
}

/** True when a rejection carries the engine's error shape. */
export function isVaultError(value: unknown): value is VaultError {
  return (
    typeof value === "object" &&
    value !== null &&
    typeof (value as VaultError).code === "string" &&
    typeof (value as VaultError).message === "string"
  );
}

// ── platform and application state ──────────────────────────────────────────

export interface PlatformReport {
  os: "windows" | "linux" | "macos";
  symlinks: {
    /** The engine created a test link and deleted it. This is the answer. */
    supported: boolean;
    probeError: string | null;
    /** Windows only. Explains the result; it does not decide it. */
    developerMode: boolean | null;
    elevated: boolean;
    /** One sentence to show the person. */
    guidance: string | null;
  };
  longPathsEnabled: boolean | null;
}

export interface Settings {
  metadataLookupsEnabled: boolean;
  /** "***" when a key is stored, null when none is. Never the value. */
  civitaiApiKey: string | null;
  hashCacheEnabled: boolean;
  scanExtensions: string[];
  minFileSizeBytes: number;
  followExtraModelPaths: boolean;
  scanOutputModelDirs: boolean;
}

export interface AppState {
  vaultRoot: string | null;
  vaultInitialized: boolean;
  installCount: number;
  platform: PlatformReport;
  settings: Settings;
  lastScanId: string | null;
  lastPlanId: string | null;
  /** Apply identifiers that need recovery before anything else may run. */
  interruptedApplies: string[];
  busy: null | { kind: "scan" | "apply" | "revert"; id: string };
}

export interface VaultInfo {
  root: string;
  createdAt: string;
  /** "C:" on Windows, the mount point on Linux. */
  volume: string;
  freeBytes: number;
  totalBytes: number;
  fileCount: number;
  totalStoredBytes: number;
  schemaVersion: number;
}

// ── installs ────────────────────────────────────────────────────────────────

export interface ExtraPath {
  section: string;
  /** The model category, after ComfyUI's own renaming. */
  category: string;
  rawCategory: string;
  path: string;
  isDefault: boolean;
  exists: boolean;
}

export interface InstallCandidate {
  valid: boolean;
  /** The real ComfyUI root, which can sit below the folder that was picked. */
  root: string | null;
  nestedDepth: number;
  markersFound: string[];
  markersMissing: string[];
  contentCheckPassed: boolean;
  otherCandidates: string[];
  version: string | null;
  versionSource: "comfyui_version.py" | "pyproject.toml" | "git" | null;
  modelsDir: string | null;
  modelsDirExists: boolean;
  extraPathsFile: string | null;
  extraPaths: ExtraPath[];
  extraPathsError: string | null;
  outputModelDirs: string[];
  /** Why valid is false. */
  reason: string | null;
}

export interface Install {
  id: string;
  label: string;
  registeredPath: string;
  root: string;
  modelsDir: string;
  version: string | null;
  versionSource: string | null;
  extraPaths: ExtraPath[];
  outputModelDirs: string[];
  addedAt: string;
  lastScanAt: string | null;
  lastScanTotals: InstallScanTotals | null;
}

export interface ModelDirNode {
  /** Relative to the root that owns it, for example "loras/style". */
  relPath: string;
  absPath: string;
  category: string;
  origin: "modelsDir" | "extraPath" | "outputDir";
  fileCount: number;
  children: ModelDirNode[];
}

// ── scan ────────────────────────────────────────────────────────────────────

export interface ScanProgress {
  scanId: string;
  phase: "enumerating" | "hashing" | "finalizing";
  installId: string | null;
  installLabel: string | null;
  filesSeen: number;
  filesToHash: number;
  filesHashed: number;
  bytesToHash: number;
  bytesHashed: number;
  bytesFromCache: number;
  currentPath: string | null;
  elapsedMs: number;
  etaMs: number | null;
}

export interface ScanTotals {
  filesSeen: number;
  movableFiles: number;
  movableBytes: number;
  uniqueContents: number;
  uniqueBytes: number;
  /** The headline number: what Apply returns if every group is applied. */
  reclaimableBytes: number;
  duplicateFiles: number;
  alreadyLinkedFiles: number;
  alreadyLinkedBytes: number;
  customNodeFiles: number;
  customNodeBytes: number;
  hfCacheFiles: number;
  hfCacheBytes: number;
  skippedFiles: number;
  errorCount: number;
  bytesRead: number;
  bytesFromCache: number;
  durationMs: number;
}

export type InstallScanTotals = ScanTotals & {
  installId: string;
  installLabel: string;
};

export interface ScanError {
  path: string;
  installId: string | null;
  code: ErrorCode;
  detail: string;
}

export interface ScanResult {
  scanId: string;
  startedAt: string;
  finishedAt: string;
  installIds: string[];
  cancelled: boolean;
  totals: ScanTotals;
  perInstall: InstallScanTotals[];
  errors: ScanError[];
}

export type Classification =
  | "movable"
  | "customNodes"
  | "huggingFaceCache"
  | "alreadyInVault"
  | "externalLink"
  | "unreadable";

export interface ScanEntry {
  absPath: string;
  relPath: string;
  installId: string;
  category: string;
  sizeBytes: number;
  sha256: string | null;
  modifiedAt: string;
  classification: Classification;
  /** How many paths hold these bytes. */
  occurrenceCount: number;
  linkTarget: string | null;
}

export interface ScanEntryFilter {
  installId?: string;
  classification?: Classification;
  category?: string;
  minSizeBytes?: number;
  nameContains?: string;
  duplicatesOnly?: boolean;
}

export interface ScanEntryPage {
  total: number;
  offset: number;
  entries: ScanEntry[];
}

// ── plan ────────────────────────────────────────────────────────────────────

export interface PlanSource {
  installId: string;
  installLabel: string;
  absPath: string;
  relPath: string;
  sameVolumeAsVault: boolean;
  chosenBecause: "sameVolume" | "onlyCopy" | "firstByPath";
}

export interface PlanLink {
  installId: string;
  installLabel: string;
  absPath: string;
  relPath: string;
  /** The name the link keeps. It can differ from the vault file's name. */
  linkName: string;
  nameDiffersFromVault: boolean;
}

export interface PlanGroup {
  groupId: string;
  sha256: string;
  sizeBytes: number;
  category: string;
  /** For example "loras/lora1.safetensors". */
  vaultRelPath: string;
  vaultNameAdjusted: boolean;
  /** The SHA-256 that already owns the plain name. */
  clashesWith: string | null;
  source: PlanSource;
  links: PlanLink[];
  occurrences: number;
  bytesFreed: number;
  singleCopy: boolean;
  crossVolume: boolean;
}

export type BlockReason =
  | "fileLocked"
  | "fileChanged"
  | "fileMissing"
  | "permissionDenied"
  | "inCustomNodes"
  | "inHuggingFaceCache"
  | "alreadyInVault"
  | "externalLink"
  | "symlinkUnsupported"
  | "vaultInsideInstall"
  | "targetExistsNotLink"
  | "notEnoughSpace"
  | "readError";

export interface BlockedRow {
  absPath: string;
  installId: string | null;
  installLabel: string | null;
  sizeBytes: number;
  sha256: string | null;
  reason: BlockReason;
  detail: string;
}

export interface PlanTotals {
  groups: number;
  groupsFreeingSpace: number;
  singleCopyGroups: number;
  nameClashes: number;
  crossVolumeGroups: number;
  bytesFreed: number;
  bytesMoved: number;
  filesMoved: number;
  linksCreated: number;
  blockedRows: number;
  blockedBytes: number;
  vaultFreeBytesAfter: number;
}

export interface ConsolidationPlan {
  planId: string;
  scanId: string;
  createdAt: string;
  vaultRoot: string;
  groups: PlanGroup[];
  blocked: BlockedRow[];
  totals: PlanTotals;
}

// ── apply ───────────────────────────────────────────────────────────────────

export interface ApplyProgress {
  applyId: string;
  phase: "preflight" | "applying" | "finalizing";
  groupIndex: number;
  groupTotal: number;
  currentGroupId: string | null;
  currentPath: string | null;
  step: "verifying" | "moving" | "linking" | "cleaning";
  bytesMoved: number;
  bytesToMove: number;
  bytesFreed: number;
  filesMoved: number;
  linksCreated: number;
  failures: number;
  elapsedMs: number;
  etaMs: number | null;
}

export interface ApplyFailure {
  groupId: string;
  absPath: string;
  reason: BlockReason;
  detail: string;
}

export type ApplyState =
  | "completed"
  | "completedWithErrors"
  | "cancelled"
  | "interrupted"
  | "reverted";

export interface ApplyResult {
  applyId: string;
  planId: string;
  state: ApplyState;
  startedAt: string;
  finishedAt: string | null;
  groupsRequested: number;
  groupsApplied: number;
  groupsFailed: number;
  bytesFreed: number;
  filesMoved: number;
  linksCreated: number;
  failures: ApplyFailure[];
  revertible: boolean;
}

export interface InterruptedApply {
  applyId: string;
  planId: string;
  startedAt: string;
  stepsDone: number;
  stepsPending: number;
  /** One sentence for the person. */
  description: string;
  affectedPaths: string[];
}

// ── links ───────────────────────────────────────────────────────────────────

export interface Link {
  id: string;
  installId: string;
  absPath: string;
  relPath: string;
  linkName: string;
  sha256: string;
  vaultRelPath: string;
  createdAt: string;
  createdBy: "apply" | "manual";
  state: "ok" | "dangling" | "replaced" | "missing";
}

// ── vault contents ──────────────────────────────────────────────────────────

export interface VaultFile {
  sha256: string;
  canonicalName: string;
  category: string;
  vaultRelPath: string;
  sizeBytes: number;
  addedAt: string;
  /** Other names this content carries inside the vault. */
  aliases: string[];
  linkCount: number;
  links: Link[];
  metadata: ModelMetadata | null;
  present: boolean;
}

export interface VaultFileFilter {
  category?: string;
  nameContains?: string;
  minSizeBytes?: number;
  orphansOnly?: boolean;
  withAliasesOnly?: boolean;
}

export interface VaultFilePage {
  total: number;
  offset: number;
  files: VaultFile[];
}

export interface NameGroupName {
  name: string;
  isCanonical: boolean;
  vaultRelPath: string;
  /** Install links that resolve through this name. */
  usedByLinks: number;
  seenInInstalls: string[];
}

export interface NameGroup {
  sha256: string;
  sizeBytes: number;
  category: string;
  canonicalName: string;
  names: NameGroupName[];
}

export interface VaultHealth {
  checkedLinks: number;
  checkedFiles: number;
  /** The link exists, the target does not. The most serious result. */
  danglingLinks: Link[];
  replacedLinks: Link[];
  missingVaultFiles: VaultFile[];
  foreignFiles: string[];
  ok: boolean;
}

// ── is a model used ─────────────────────────────────────────────────────────

export interface UsageMatch {
  installId: string;
  installLabel: string;
  workflowPath: string;
  workflowName: string;
}

export interface UsageResult {
  name: string;
  used: boolean;
  matches: UsageMatch[];
  /**
   * Always the same sentence, saying what the check actually did. The interface
   * must show it next to the answer.
   */
  method: string;
}

// ── metadata ────────────────────────────────────────────────────────────────

export interface ModelMetadata {
  sha256: string;
  source: "civitai";
  fetchedAt: string;
  found: boolean;
  modelName: string | null;
  modelType: string | null;
  versionName: string | null;
  baseModel: string | null;
  triggerWords: string[];
  nsfw: boolean;
  nsfwLevel: number;
  civitaiModelId: number | null;
  civitaiVersionId: number | null;
  pageUrl: string | null;
  downloadUrl: string | null;
  previewImageUrls: string[];
  /** The hash matched more than one model version. */
  ambiguous: boolean;
}

// ── running programs ────────────────────────────────────────────────────────

export interface RunningComfy {
  pid: number;
  name: string;
  exePath: string | null;
  cwd: string | null;
  commandLine: string[];
  matchedInstallIds: string[];
  matchReason: "exeUnderRoot" | "cwdUnderRoot" | "argUnderRoot";
}

export interface LockState {
  path: string;
  locked: boolean;
  /** False on systems without mandatory locking. Then locked is no guarantee. */
  checkable: boolean;
  detail: string | null;
}

// ── walking folders, for the picker ─────────────────────────────────────────

/**
 * One folder in the picker's tree.
 *
 * The person never types a path, so the picker has to walk the disk. The
 * contract has no command for this yet: comfyvault-core has been asked either
 * to add `list_directory` and `create_directory`, or to grant the Tauri file
 * system plugin permission to read directories, which is what the client does
 * today. Either way it is one file to change.
 */
export interface DirectoryEntry {
  path: string;
  name: string;
  isDrive: boolean;
  readable: boolean;
  hasChildren: boolean;
}

// ── the port ────────────────────────────────────────────────────────────────

export type Unsubscribe = () => void;

/**
 * Everything the interface can ask of the engine, one method per command in
 * docs/IPC-CONTRACT.md. The Tauri client and the development engine both
 * implement it, so no screen knows which one it has.
 */
export interface Engine {
  // platform and state
  getPlatformReport(): Promise<PlatformReport>;
  getAppState(): Promise<AppState>;
  selectVault(path: string, createIfMissing: boolean): Promise<VaultInfo>;
  getSettings(): Promise<Settings>;
  updateSettings(patch: Partial<Settings>): Promise<Settings>;

  // installs
  validateInstallPath(path: string): Promise<InstallCandidate>;
  registerInstall(path: string, label?: string): Promise<Install>;
  listInstalls(): Promise<Install[]>;
  refreshInstall(id: string): Promise<Install>;
  updateInstall(id: string, label: string): Promise<Install>;
  unregisterInstall(
    id: string,
  ): Promise<{ removed: true; linksLeftInPlace: number }>;
  listInstallModelDirs(id: string): Promise<ModelDirNode[]>;

  // walking folders, for the picker
  /** Children of a folder. Pass null for the drives. */
  listDirectory(path: string | null): Promise<DirectoryEntry[]>;
  createDirectory(parent: string, name: string): Promise<{ path: string }>;

  // scan
  startScan(installIds?: string[]): Promise<{ scanId: string }>;
  cancelScan(scanId: string): Promise<{ cancelled: true }>;
  getLastScan(): Promise<ScanResult | null>;
  getScanEntries(args: {
    scanId: string;
    offset: number;
    limit: number;
    filter?: ScanEntryFilter;
  }): Promise<ScanEntryPage>;
  onScanProgress(fn: (p: ScanProgress) => void): Unsubscribe;
  onScanDone(fn: (r: ScanResult) => void): Unsubscribe;
  onScanError(fn: (e: VaultError) => void): Unsubscribe;

  // plan
  buildPlan(scanId: string): Promise<ConsolidationPlan>;
  getPlan(planId: string): Promise<ConsolidationPlan>;

  // apply
  startApply(args: {
    planId: string;
    groupIds: string[];
    verify?: "sizeAndMtime" | "rehash";
    stopOnError?: boolean;
  }): Promise<{ applyId: string }>;
  cancelApply(applyId: string): Promise<{ cancelled: true }>;
  getApplyResult(applyId: string): Promise<ApplyResult>;
  listApplies(): Promise<ApplyResult[]>;
  getInterruptedApplies(): Promise<InterruptedApply[]>;
  resumeApply(applyId: string): Promise<{ applyId: string }>;
  revertApply(applyId: string): Promise<{ applyId: string }>;
  onApplyProgress(fn: (p: ApplyProgress) => void): Unsubscribe;
  onApplyDone(fn: (r: ApplyResult) => void): Unsubscribe;
  onApplyError(fn: (e: VaultError) => void): Unsubscribe;
  onRevertProgress(fn: (p: ApplyProgress) => void): Unsubscribe;
  onRevertDone(fn: (r: ApplyResult) => void): Unsubscribe;
  onRevertError(fn: (e: VaultError) => void): Unsubscribe;

  // links
  createLink(args: {
    installId: string;
    sha256: string;
    relativeDir: string;
    linkName?: string;
    createDir?: boolean;
  }): Promise<Link>;
  removeLink(linkId: string): Promise<{ removed: true }>;
  createModelFolder(
    installId: string,
    relativeDir: string,
  ): Promise<{ absPath: string; created: boolean }>;
  listLinks(filter?: {
    installId?: string;
    sha256?: string;
    state?: Link["state"];
  }): Promise<Link[]>;

  // vault contents
  listVaultFiles(args: {
    offset: number;
    limit: number;
    filter?: VaultFileFilter;
    sort?: "name" | "size" | "addedAt" | "linkCount";
    descending?: boolean;
  }): Promise<VaultFilePage>;
  listNameGroups(): Promise<NameGroup[]>;
  setCanonicalName(sha256: string, name: string): Promise<VaultFile>;
  removeAlias(sha256: string, name: string): Promise<{ removed: true }>;
  listOrphans(): Promise<VaultFile[]>;
  deleteVaultFile(
    sha256: string,
    confirm: string,
  ): Promise<{ deleted: true; bytesFreed: number }>;
  checkVaultHealth(): Promise<VaultHealth>;

  // usage and metadata
  checkModelUsage(names: string[], installIds?: string[]): Promise<UsageResult[]>;
  getMetadata(sha256: string, refresh?: boolean): Promise<ModelMetadata | null>;
  fetchMetadataBatch(
    sha256: string[],
    refresh?: boolean,
  ): Promise<ModelMetadata[]>;

  // running programs
  getRunningComfy(): Promise<RunningComfy[]>;
  checkLockedFiles(paths: string[]): Promise<LockState[]>;

  // the window itself
  openExternal(target: string): Promise<void>;
  revealInFileManager(path: string): Promise<void>;
  windowMinimize(): Promise<void>;
  windowToggleMaximize(): Promise<void>;
  windowClose(): Promise<void>;
}
