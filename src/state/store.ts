/**
 * The one place the interface keeps what it knows.
 *
 * Everything the screens print is either something the engine returned, held
 * here as it came, or a memo derived from it. Nothing is stored twice: the
 * groups, the totals and the Apply gate all come from the plan, the platform
 * report and the set of groups the person unticked.
 */

import {
  batch,
  createContext,
  createMemo,
  createSignal,
  onCleanup,
  useContext,
  type Accessor,
} from "solid-js";
import { createStore, produce, type SetStoreFunction } from "solid-js/store";

import {
  buildInstallViews,
  buildPlanView,
  type InstallView,
  type PlanView,
} from "~/domain/view";
import {
  applyGate,
  selectionFor,
  type ApplyGate,
  type MachineFacts,
  type Selection,
} from "~/domain/selection";
import { volumeLabel } from "~/domain/drives";
import { nameCardsOf, type NameCard } from "~/domain/names";
import { runLookups } from "~/domain/lookup";
import { createDownloadState, type DownloadState } from "~/state/download";
import type {
  ApplyProgress,
  ApplyRecord,
  AppState,
  ConsolidationPlan,
  ContentRow,
  DirectoryEntry,
  Download,
  DriveInfo,
  Engine,
  HiddenNameCard,
  Install,
  InstallCandidate,
  InterruptedApply,
  LinkFolder,
  LinkRecord,
  NameGroup,
  RevertProgress,
  RunningComfy,
  ScanProgress,
  ScanRecord,
  UnifyPlan,
  UsageResult,
  VaultFile,
  VaultHealth,
  VaultInfo,
} from "~/ipc/contract";
import { isVaultError, nothingWasSearched } from "~/ipc/contract";

export type Screen =
  | "home"
  | "library"
  | "consolidate"
  | "cleanup"
  | "download"
  | "settings";

export type LibrarySort = "name" | "size" | "links";

export interface LibraryView {
  query: string;
  category: string;
  unusedOnly: boolean;
  sort: LibrarySort;
  selected: string | null;
  drawerOpen: boolean;
}

export interface Toast {
  message: string;
  tone: "ok" | "bad";
  /** A button in the toast, such as Undo. */
  action?: { label: string; run: () => void };
}

export interface TreeNode extends DirectoryEntry {
  depth: number;
  /** The person created it in this picker a moment ago. */
  isNew: boolean;
  /** A drive root, which takes a different glyph. */
  isDrive: boolean;
  /** Null until the engine has been asked what is inside. */
  hasChildren: boolean | null;
  /** Why the engine refused to open it, once it has. */
  refusal: string | null;
}

export interface NewFolderDraft {
  parent: string;
  name: string;
  error: string | null;
  saving: boolean;
}

export type PickerPurpose = "install" | "vault";

export interface PickerModal {
  kind: "picker";
  purpose: PickerPurpose;
  nodes: TreeNode[];
  expanded: string[];
  loading: string[];
  picked: string | null;
  /** What the engine said about the picked folder, when it was asked. */
  candidate: InstallCandidate | null;
  checking: boolean;
  newFolder: NewFolderDraft | null;
  /** Which install is being re-pointed, when the person pressed Edit. */
  replacing: string | null;
  /**
   * Why the last press of the button did not happen. It stays in front of the
   * person until they act, because a toast can be missed and an answer to
   * something they just pressed is not allowed to go by unnoticed.
   */
  error: string | null;
  /**
   * The engine refused the picked folder as the vault because it already
   * holds files. The way out is a new folder, so the picker points there.
   */
  folderHasFiles: boolean;
  /** How many installs this visit to the picker has registered. */
  added: number;
  /** What the last press added: its root, or "3 installs". */
  lastAdded: string | null;
}

export type ConfirmLine = ReadonlyArray<{ text: string; emph?: boolean }>;

export interface ConfirmModal {
  kind: "confirm";
  title: string;
  body: ConfirmLine[];
  /** Paths the person must see before confirming, one per line. */
  list: readonly string[];
  /** What follows the paths, when a line only makes sense after them. */
  after: ConfirmLine[];
  /** Null when the action is refused before it starts, so none is offered. */
  cta: string | null;
  /** The action is shown, but cannot be pressed yet. */
  ctaOff: boolean;
  action: () => Promise<void> | void;
  running: boolean;
  /** The heading over `error`. */
  errorHead: string;
  error: string | null;
  /** What the engine named as being in the way. One entry per line. */
  errorDetail: readonly string[];
}

/** The folder a new link goes in. See modals/linkfolder.tsx. */
export interface LinkFolderModal {
  kind: "linkFolder";
  /** Null while the person still picks the install, from the Library. */
  installId: string | null;
  category: string;
  fileName: string;
  /** From the Library: the model the chooser links itself. */
  sha256: string | null;
  selected: string | null;
  /** The folders under each folder, "" for the roots. */
  folders: Record<string, LinkFolder[]>;
  expanded: string[];
  loading: string[];
  /** New folders drawn in the tree. They are made when the link is. */
  added: string[];
  naming: { draft: string; error: string | null } | null;
  error: string | null;
  working: boolean;
  /** From Download: what to do with the folder. The card keeps it. */
  onUse: ((dir: string) => void) | null;
}

/** Giving one model one name in every install. */
export interface UnifyModal {
  kind: "unify";
  sha256: string;
  name: string;
  /** Null while the engine works out the plan. */
  plan: UnifyPlan | null;
  working: boolean;
  error: string | null;
}

export type Modal = PickerModal | ConfirmModal | LinkFolderModal | UnifyModal;

export interface AppStore {
  readonly engine: Engine;

  readonly appState: Accessor<AppState | null>;
  readonly vault: Accessor<VaultInfo | null>;
  readonly installs: Accessor<readonly Install[]>;
  readonly scan: Accessor<ScanRecord | null>;
  readonly plan: Accessor<ConsolidationPlan | null>;
  readonly vaultFiles: Accessor<readonly VaultFile[]>;
  readonly nameGroups: Accessor<readonly NameGroup[]>;
  /** The cards for models the installs use more than one name for, less the hidden ones. */
  readonly nameCards: Accessor<readonly NameCard[]>;
  readonly hiddenNameCards: Accessor<readonly HiddenNameCard[]>;
  /** Cleanup's line once a name was changed, until the person leaves Cleanup. */
  readonly nameResult: Accessor<string | null>;
  readonly setNameResult: (line: string | null) => void;
  readonly orphans: Accessor<readonly VaultFile[]>;
  readonly health: Accessor<VaultHealth | null>;
  /**
   * Links that point at a file that is not there. ComfyUI lists one in its
   * dropdown and then fails to load it, and a node that re-downloads the
   * "missing" model writes straight through it into the vault, so this is the
   * first thing the interface says.
   */
  readonly danglingLinks: Accessor<readonly LinkRecord[]>;
  readonly running: Accessor<readonly RunningComfy[]>;
  readonly interrupted: Accessor<readonly InterruptedApply[]>;
  /**
   * The last run, when it was cut off and nothing is running it now.
   *
   * Measured against the real engine: a run cut off by a crash comes back as
   * state `running`, and `getInterruptedApplies` lists it. Shown as finished,
   * it read "undefined" in its header and offered only Undo, so the promise that
   * a run cut off can be finished had no door.
   */
  readonly cutOffRun: Accessor<InterruptedApply | null>;
  readonly lastApply: Accessor<ApplyRecord | null>;
  /**
   * The last run, while it is still the latest thing that happened: until a
   * scan finishes after it. Then the new scan's plan is what Consolidate shows,
   * so a second run can be planned without undoing the first.
   *
   * A run cut off or an undo stopped part way stays on screen whatever was
   * scanned since, because each has to be settled before anything else.
   *
   * Measured against the real engine: after a finished run, a model downloaded
   * into two installs and a scan give a plan of that one model, and applying it
   * finishes as a second run.
   */
  readonly runOnScreen: Accessor<ApplyRecord | null>;
  /**
   * The plan the last run actually applied, as the engine stored it.
   *
   * Not a fresh one built from the same scan. After a run every consolidated
   * path differs from what that scan recorded, so a rebuilt plan reports the
   * whole tree as changed under it, and a screen saying the run succeeded
   * printed that as files it could not move.
   */
  readonly appliedPlan: Accessor<ConsolidationPlan | null>;
  /**
   * The last scan is the one the last run consumed, so it describes the world
   * as it was before that run.
   *
   * A run does not scan. Every figure taken from that scan, the models on
   * disk, what each install holds, what could be reclaimed, was true before
   * the run and is not afterwards, and printing it as current sits it beside a
   * panel saying the space has already come back.
   */
  readonly scanPredatesRun: Accessor<boolean>;
  /**
   * The last scan was taken before an undo touched the disk, whether that undo
   * finished or stopped part way.
   *
   * Measured against the real engine: an undo does not scan either. When the
   * last scan was taken after the run, a plan built from it after the undo
   * finds nothing to consolidate, on a tree that holds every duplicate again.
   */
  readonly scanPredatesUndo: Accessor<boolean>;
  /**
   * The last run was set aside, and the last scan was taken before that run
   * started, so it describes the installs as they were before the run's links.
   */
  readonly scanPredatesSetAside: Accessor<boolean>;
  readonly usage: Accessor<ReadonlyMap<string, UsageResult>>;
  readonly usageMethod: Accessor<string | null>;

  /** Where the background Civitai lookup is. */
  readonly lookup: Accessor<LookupStatus>;
  readonly scanProgress: Accessor<ScanProgress | null>;
  readonly applyProgress: Accessor<ApplyProgress | null>;
  /** An undo while it runs, which reports in its own shape. */
  readonly revertProgress: Accessor<RevertProgress | null>;
  /**
   * Why a link can be neither made nor removed now: a consolidation or an
   * undo runs, and the engine refuses link changes until it ends. Null when
   * nothing stops them.
   */
  readonly linkBusy: Accessor<string | null>;
  /**
   * Why a model can be neither given one name nor deleted with its links now:
   * any long job runs, a scan too. Null when nothing stops it.
   */
  readonly anyBusy: Accessor<string | null>;
  readonly ready: Accessor<boolean>;
  readonly failure: Accessor<string | null>;

  readonly planView: Accessor<PlanView | null>;
  /** The models in the vault. The Library manages the vault, not the installs. */
  readonly library: Accessor<readonly ContentRow[]>;
  /** Every model the engine knows, in the vault or still out in the installs. */
  readonly contents: Accessor<readonly ContentRow[]>;
  /** How many models the vault holds in total, beyond the page loaded. */
  readonly libraryTotal: Accessor<number>;
  /** True when there was no saved workflow file to search at all. */
  readonly nothingSearched: Accessor<boolean>;
  readonly installViews: Accessor<readonly InstallView[]>;
  readonly selection: Accessor<Selection>;
  readonly gate: Accessor<ApplyGate>;
  readonly hasInstalls: Accessor<boolean>;
  /** A vault folder exists. Almost nothing works until it does. */
  readonly hasVault: Accessor<boolean>;
  /**
   * The vault's drive as a person writes it, "C:".
   *
   * The engine reports whatever the operating system calls that volume, which
   * on Windows is the drive root with a trailing separator. Every screen reads
   * it from here so none of them prints it raw or compares it against a path.
   */
  readonly vaultVolume: Accessor<string>;
  /**
   * An install folder the person has chosen while there was still no vault to
   * record it in. The engine refuses to register one before a vault exists, so
   * the interface holds it until the vault folder is chosen and then registers
   * it. It lives only in this window until then.
   */
  /**
   * Every drive on this computer. Read before a vault exists, because which
   * drives there are and what room each has is what makes the vault-folder
   * choice an informed one.
   */
  readonly drives: Accessor<readonly DriveInfo[]>;
  /**
   * Setup is over once the person starts the first scan. Until then they are
   * still adding installs, and one scan reads every install they add.
   */
  readonly setupDone: Accessor<boolean>;
  /**
   * What is still to be done before setup is over, said as the thing it is.
   * Null once it is.
   */
  readonly missingStep: Accessor<string | null>;
  /**
   * The roots added by the last visit to the install picker, marked in the
   * setup list until the next scan starts so the person sees what they added.
   */
  readonly freshInstalls: Accessor<readonly string[]>;
  /**
   * No scan has read the installs: there is none, or the last one was
   * cancelled. Measured against the real engine: a cancelled scan is kept as
   * the last scan, with every total at zero, so its figures are not sizes.
   */
  readonly nothingRead: Accessor<boolean>;
  readonly setFreshInstalls: (roots: readonly string[]) => void;
  readonly unusedCount: Accessor<number>;

  readonly screen: Accessor<Screen>;
  readonly toast: Accessor<Toast | null>;
  readonly unticked: Accessor<ReadonlySet<string>>;
  readonly showAllDuplicates: Accessor<boolean>;
  readonly showSingles: Accessor<boolean>;
  readonly categoryMenuOpen: Accessor<boolean>;

  readonly lib: LibraryView;
  readonly setLib: SetStoreFunction<LibraryView>;
  /** The downloads, and the card for the address being read. */
  readonly dl: DownloadState;
  readonly modal: Accessor<Modal | null>;
  readonly setModal: (modal: Modal | null) => void;
  readonly patchModal: (fn: (modal: Modal) => void) => void;

  readonly actions: Actions;
}

export interface Actions {
  go(screen: Screen): void;
  showToast(message: string, tone?: "ok" | "bad", action?: Toast["action"]): void;
  refresh(): Promise<void>;
  /** Run an engine call and turn any refusal into a message the person can read. */
  run(what: () => Promise<unknown>, onOk?: string): Promise<boolean>;
  toggleGroup(groupId: string): void;
  setShowAllDuplicates(on: boolean): void;
  setShowSingles(on: boolean): void;
  setCategoryMenuOpen(on: boolean): void;
}

const StoreContext = createContext<AppStore>();

export function useApp(): AppStore {
  const app = useContext(StoreContext);
  if (!app) throw new Error("useApp was called outside the app");
  return app;
}

export const AppProvider = StoreContext.Provider;

/** Turn whatever an engine call rejected with into one readable sentence. */
export function messageOf(error: unknown): string {
  if (isVaultError(error)) return error.message;
  if (error instanceof Error) return error.message;
  return String(error);
}

/**
 * The lines an engine refusal carries beyond its sentence. A refusal that names
 * the paths in the way is the difference between knowing what happened and
 * knowing what to do about it, so the interface never drops them.
 */
export function detailOf(error: unknown): readonly string[] {
  if (!isVaultError(error) || !error.detail) return [];
  return error.detail
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line.length > 0);
}

/**
 * When an undo last touched the disk, on any run.
 *
 * `lastUndoStepAt` is written before every undo step, so it covers an undo
 * that stopped part way as well as one that finished. A reverted record from
 * an older build has none, and there the undo's own finish time stands in:
 * the engine stamps `finishedAt` again when it undoes a run.
 *
 * Measured against the real engine, in the person's order (run, scan, undo,
 * stop): the scan finished at .376, the undo's last step began at .388, so the
 * scan is out of date. A scan after the stop finished at .681 and is current.
 */
function latestUndo(applies: readonly ApplyRecord[]): string | null {
  let latest: string | null = null;
  for (const a of applies) {
    const at = a.lastUndoStepAt ?? (a.state === "reverted" ? a.finishedAt : null);
    if (at === null) continue;
    if (latest === null || Date.parse(at) > Date.parse(latest)) latest = at;
  }
  return latest;
}

/** True when this scan was taken before a run that has since been set aside. */
function predatesSetAside(run: ApplyRecord | null, scan: ScanRecord | null): boolean {
  return (
    run !== null &&
    scan !== null &&
    run.state === "setAside" &&
    Date.parse(scan.finishedAt) < Date.parse(run.startedAt)
  );
}

/** True when an undo finished after this scan, so the scan is out of date. */
function overtakenByUndo(undoneAt: string | null, scan: ScanRecord | null): boolean {
  return (
    undoneAt !== null &&
    scan !== null &&
    Date.parse(undoneAt) > Date.parse(scan.finishedAt)
  );
}

/**
 * The background Civitai lookup.
 *
 * `unreachable` holds the engine's own words for why: the network is down, or
 * Civitai asked the app to slow down. It is not retried until the next scan or
 * until the switch is turned on again, so a refusal is never answered by asking
 * again straight away.
 */
export type LookupStatus =
  | { kind: "idle" }
  | { kind: "running"; asked: number; total: number }
  | { kind: "done" }
  | { kind: "unreachable"; message: string };

export function createAppStore(engine: Engine): AppStore {
  const [appState, setAppState] = createSignal<AppState | null>(null);
  const [vault, setVault] = createSignal<VaultInfo | null>(null);
  const [installs, setInstalls] = createSignal<readonly Install[]>([]);
  const [scan, setScan] = createSignal<ScanRecord | null>(null);
  const [plan, setPlan] = createSignal<ConsolidationPlan | null>(null);
  const [vaultFiles, setVaultFiles] = createSignal<readonly VaultFile[]>([]);
  const [nameGroups, setNameGroups] = createSignal<readonly NameGroup[]>([]);
  const [hiddenNameCards, setHiddenNameCards] = createSignal<readonly HiddenNameCard[]>([]);
  const nameCards = createMemo(() => nameCardsOf(nameGroups(), hiddenNameCards()));
  const [nameResult, setNameResult] = createSignal<string | null>(null);
  const [orphans, setOrphans] = createSignal<readonly VaultFile[]>([]);
  const [health, setHealth] = createSignal<VaultHealth | null>(null);
  const [running, setRunning] = createSignal<readonly RunningComfy[]>([]);
  const [interrupted, setInterrupted] = createSignal<readonly InterruptedApply[]>([]);
  const [lastApply, setLastApply] = createSignal<ApplyRecord | null>(null);
  const [appliedPlan, setAppliedPlan] = createSignal<ConsolidationPlan | null>(null);
  const scanPredatesRun = createMemo(() => {
    const ran = appliedPlan();
    const last = scan();
    return ran !== null && last !== null && ran.scanId === last.scanId;
  });
  /** When the most recent undo finished, as the engine recorded it. */
  const [lastUndoneAt, setLastUndoneAt] = createSignal<string | null>(null);
  const cutOffRun = createMemo(() => {
    const run = lastApply();
    if (!run || appState()?.busy?.id === run.applyId) return null;
    return interrupted().find((i) => i.applyId === run.applyId) ?? null;
  });
  const runOnScreen = createMemo(() => {
    const run = lastApply();
    if (!run) return null;
    if (run.state === "partlyReverted" || cutOffRun()) return run;
    // Set aside and not back yet: its record counts nothing the person can use.
    if (run.state === "setAside") return null;
    const last = scan();
    const scannedSince =
      last !== null &&
      run.finishedAt !== null &&
      Date.parse(last.finishedAt) > Date.parse(run.finishedAt);
    return scannedSince ? null : run;
  });
  const scanPredatesUndo = createMemo(() => overtakenByUndo(lastUndoneAt(), scan()));
  const scanPredatesSetAside = createMemo(() => predatesSetAside(lastApply(), scan()));
  const [usage, setUsage] = createSignal<ReadonlyMap<string, UsageResult>>(new Map());
  const [library, setLibrary] = createSignal<readonly ContentRow[]>([]);
  /** Every model the engine knows, in the vault or still out in the installs. */
  const [contents, setContents] = createSignal<readonly ContentRow[]>([]);
  const [libraryTotal, setLibraryTotal] = createSignal(0);
  const [nothingSearched, setNothingSearched] = createSignal(false);

  const [lookup, setLookup] = createSignal<LookupStatus>({ kind: "idle" });
  const [scanProgress, setScanProgress] = createSignal<ScanProgress | null>(null);
  const [applyProgress, setApplyProgress] = createSignal<ApplyProgress | null>(null);
  const [revertProgress, setRevertProgress] = createSignal<RevertProgress | null>(null);
  const linkBusy = createMemo(() => {
    const kind = appState()?.busy?.kind;
    if (revertProgress() !== null || kind === "revert") return "Wait for the undo to finish";
    if (applyProgress() !== null || kind === "apply") return "Wait for the consolidation to finish";
    return null;
  });
  const anyBusy = createMemo(() => {
    if (linkBusy()) return linkBusy();
    if (scanProgress() !== null || appState()?.busy?.kind === "scan") {
      return "Wait for the scan to finish";
    }
    return null;
  });
  const [ready, setReady] = createSignal(false);
  const [failure, setFailure] = createSignal<string | null>(null);

  const [screen, setScreen] = createSignal<Screen>("home");
  const [toast, setToast] = createSignal<Toast | null>(null);
  const [unticked, setUnticked] = createSignal<ReadonlySet<string>>(new Set<string>());
  const [showAllDuplicates, setShowAllDuplicates] = createSignal(false);
  const [showSingles, setShowSingles] = createSignal(false);
  const [categoryMenuOpen, setCategoryMenuOpen] = createSignal(false);
  const [modal, setModalStore] = createStore<{ current: Modal | null }>({
    current: null,
  });

  const [lib, setLib] = createStore<LibraryView>({
    query: "",
    category: "all",
    unusedOnly: false,
    sort: "size",
    selected: null,
    drawerOpen: false,
  });

  const dl = createDownloadState(engine, (error) => messageOf(error));

  let toastTimer: ReturnType<typeof setTimeout> | null = null;
  const showToast = (message: string, tone: "ok" | "bad" = "ok", action?: Toast["action"]) => {
    if (toastTimer) clearTimeout(toastTimer);
    setToast({ message, tone, ...(action ? { action } : {}) });
    toastTimer = setTimeout(() => setToast(null), 3600);
  };

  // ── derived ───────────────────────────────────────────────────────────────

  const planView = createMemo<PlanView | null>(() => {
    const current = plan();
    if (!current) return null;
    return buildPlanView(current, scan()?.totals ?? null);
  });

  const runningInstallIds = createMemo(
    () => new Set(running().flatMap((p) => p.matchedInstallIds)),
  );

  const installViews = createMemo<readonly InstallView[]>(() =>
    buildInstallViews(installs(), plan(), runningInstallIds()),
  );

  const selection = createMemo<Selection>(() => selectionFor(plan(), unticked()));

  const machineFacts = createMemo<MachineFacts>(() => ({
    platform: appState()?.platform ?? null,
    running: running(),
    interrupted: interrupted(),
  }));

  const gate = createMemo<ApplyGate>(() =>
    applyGate(machineFacts(), selection(), appState()?.busy ?? null),
  );

  const [drives, setDrives] = createSignal<readonly DriveInfo[]>([]);
  const hasInstalls = createMemo(() => installs().length > 0);
  const hasVault = createMemo(() => appState()?.vaultInitialized === true);
  const vaultVolume = createMemo(() => volumeLabel(vault()?.volume) || "C:");
  // Setup holds until the first scan reports progress. In the moment before
  // that, the setup screen stays and its scan button waits on the engine.
  const setupDone = createMemo(
    () => hasInstalls() && hasVault() && (scan() !== null || scanProgress() !== null),
  );
  const missingStep = createMemo(() => {
    if (setupDone()) return null;
    // The vault comes first because an install cannot be recorded without one,
    // so there is no state where installs are set and the vault is not.
    if (!hasVault()) return "Choose where the vault goes, then register a ComfyUI install.";
    if (!hasInstalls()) return "Register a ComfyUI install. The vault folder is already set.";
    return "Your installs are registered and nothing has been read yet.";
  });
  const [freshInstalls, setFreshInstalls] = createSignal<readonly string[]>([]);
  const nothingRead = createMemo(() => {
    const last = scan();
    return last === null || last.cancelled;
  });

  const danglingLinks = createMemo<readonly LinkRecord[]>(
    () => health()?.danglingLinks ?? [],
  );

  /**
   * How many models no saved workflow names. Zero when nothing was searched,
   * because "not checked" is not an answer a person should act on.
   */
  const unusedCount = createMemo(() => {
    if (nothingSearched()) return 0;
    const answers = usage();
    if (answers.size === 0) return 0;
    return contents().filter((row) => answers.get(row.name)?.used === false).length;
  });

  const usageMethod = createMemo(() => {
    for (const answer of usage().values()) return answer.method;
    return null;
  });

  // ── loading ───────────────────────────────────────────────────────────────

  const loadUsage = async (rows: readonly ContentRow[]) => {
    const names = [...new Set(rows.flatMap((row) => [row.name, ...row.aliases]))];
    if (names.length === 0) {
      batch(() => {
        setUsage(new Map());
        setNothingSearched(false);
      });
      return;
    }
    try {
      const answers = await engine.checkModelUsage(names);
      batch(() => {
        setUsage(new Map(answers.map((a) => [a.name, a])));
        setNothingSearched(answers.length > 0 && answers.every(nothingWasSearched));
      });
    } catch {
      // Whether a model is named in a workflow is useful, not essential. The
      // screen says when the answer is missing rather than pretending.
      setUsage(new Map());
    }
  };

  /**
   * A call that may turn out to be one the engine only answers with a vault
   * open, with what to use when it says "not yet".
   *
   * One refusal inside a batch used to reject the whole batch and take the
   * screen down with it. A command becoming conditional is a normal thing for
   * the engine to do, and when it happens the interface should lose that one
   * answer, not the window.
   */
  const orNotYet = async <T>(call: Promise<T>, fallback: T): Promise<T> => {
    try {
      return await call;
    } catch (error) {
      if (isVaultError(error) && error.code === "notInitialized") return fallback;
      throw error;
    }
  };

  const refresh = async () => {
    try {
      const state = await engine.getAppState();
      // It answers whether or not a vault exists, and the first screen needs it
      // most when one does not.
      setDrives(await orNotYet(engine.listDrives(), []));

      // Before a vault folder exists there is nothing to read and every other
      // command refuses, so this is where the first run stops. Asking anyway
      // is what greeted a new person with a failure screen on a program they
      // had not used yet.
      if (!state.vaultInitialized) {
        batch(() => {
          setAppState(state);
          setVault(null);
          setInstalls([]);
          setScan(null);
          setPlan(null);
          setVaultFiles([]);
          setNameGroups([]);
          setHiddenNameCards([]);
          setOrphans([]);
          setHealth(null);
          setLibrary([]);
          setContents([]);
          setLibraryTotal(0);
          setRunning([]);
          setInterrupted([]);
          setLastApply(null);
          setAppliedPlan(null);
          setLastUndoneAt(null);
          setUsage(new Map());
          setNothingSearched(false);
          dl.replace([]);
          setFailure(null);
          setReady(true);
        });
        return;
      }

      const [installList, lastScan, interruptedList, applies, runningList] =
        await Promise.all([
          orNotYet(engine.listInstalls(), [] as Install[]),
          orNotYet(engine.getLastScan(), null),
          orNotYet(engine.getInterruptedApplies(), [] as InterruptedApply[]),
          orNotYet(engine.listApplies(), [] as ApplyRecord[]),
          orNotYet(engine.getRunningComfy(), [] as RunningComfy[]),
        ]);

      const vaultInfo = await orNotYet(engine.getVaultInfo(), null);

      // A scan an undo has overtaken does not describe the installs. Measured
      // against the real engine: a plan built from a scan taken after the run,
      // once the run is undone, finds nothing to consolidate on a tree that
      // holds every duplicate again. Nothing is built from it, so no screen can
      // print it.
      const undoneAt = latestUndo(applies);
      const scanOvertaken =
        overtakenByUndo(undoneAt, lastScan) ||
        predatesSetAside(applies.find((a) => a.state !== "reverted") ?? null, lastScan);
      const nextPlan =
        lastScan && !lastScan.cancelled && !scanOvertaken
          ? await orNotYet(engine.buildPlan(lastScan.scanId), null)
          : null;

      const [files, groups, orphanList, vaultHealth, contents, inVault, downloadList, hidden] = await Promise.all([
        orNotYet(engine.listVaultFiles({ offset: 0, limit: 1000 }), {
          total: 0,
          offset: 0,
          files: [] as VaultFile[],
        }),
        orNotYet(engine.listNameGroups(), [] as NameGroup[]),
        orNotYet(engine.listOrphans(), [] as VaultFile[]),
        orNotYet(engine.checkVaultHealth(), null),
        orNotYet(engine.listContents({ offset: 0, limit: 1000, sort: "size" }), {
          total: 0,
          offset: 0,
          rows: [] as ContentRow[],
          scanId: null,
        }),
        // The Library manages the vault, not the installs: only what is in it.
        orNotYet(
          engine.listContents({ offset: 0, limit: 1000, sort: "size", filter: { inVault: true } }),
          { total: 0, offset: 0, rows: [] as ContentRow[], scanId: null },
        ),
        orNotYet(engine.listDownloads(), [] as Download[]),
        orNotYet(engine.getHiddenNameCards(), [] as HiddenNameCard[]),
      ]);

      batch(() => {
        setAppState(state);
        setVault(vaultInfo);
        setInstalls(installList);
        setScan(lastScan);
        setPlan(nextPlan);
        setVaultFiles(files.files);
        setNameGroups(groups);
        setHiddenNameCards(hidden);
        setOrphans(orphanList);
        setHealth(vaultHealth);
        setContents(contents.rows);
        setLibrary(inVault.rows);
        setLibraryTotal(inVault.total);
        dl.replace(downloadList);
        setRunning(runningList);
        setInterrupted(interruptedList);
        setLastApply(applies.find((a) => a.state !== "reverted") ?? null);
        setLastUndoneAt(undoneAt);
        setFailure(null);
        setReady(true);
      });

      // The plan that ran, read back as the engine stored it. It is the only
      // authority on what that run could not move.
      const ran = applies.find((a) => a.state !== "reverted") ?? null;
      setAppliedPlan(
        ran ? await orNotYet(engine.getPlan(ran.planId), null) : null,
      );

      await loadUsage(contents.rows);

      const lookupsOn = state.settings.metadataLookupsEnabled === true;
      if (lookupsOn && !lookupsWereOn) lookupDue = true;
      lookupsWereOn = lookupsOn;
      if (lookupDue) {
        lookupDue = false;
        void lookUpMetadata();
      }
    } catch (error) {
      batch(() => {
        // "No vault folder is open yet" is not a failure. It is what a program
        // nobody has used yet says, and the answer to it is the setup screen,
        // not an apology with a button that cannot do anything.
        setFailure(isVaultError(error) && error.code === "notInitialized"
          ? null
          : messageOf(error));
        setReady(true);
      });
    }
  };

  // ── the Civitai lookup ────────────────────────────────────────────────────

  /**
   * Set after every scan, and when the switch is seen on where it was not
   * before, which includes the first load with the switch on.
   */
  let lookupDue = false;
  let lookupsWereOn = false;
  let lookupPass = 0;
  let disposed = false;

  /** Every content the cache has no answer for, across every page. */
  const unanswered = async (): Promise<string[]> => {
    const out: string[] = [];
    for (let offset = 0; ; offset += 1000) {
      const page = await engine.listContents({ offset, limit: 1000, sort: "size" });
      for (const row of page.rows) if (row.metadata === null) out.push(row.sha256);
      if (page.rows.length === 0 || offset + page.rows.length >= page.total) return out;
    }
  };

  /**
   * Asks Civitai about every file it has not answered for, in the background.
   *
   * `refresh` is passed only for hashes with no cached answer, so it costs no
   * extra request, and it is what makes the engine say so when the network is
   * down rather than answer "not found" for everything. Measured against the
   * real engine and the live service: a known model comes back found with its
   * name, base model, page and pictures; an unknown hash comes back found:
   * false and is cached, so it is not asked again; with the switch off nothing
   * goes out.
   */
  const lookUpMetadata = async () => {
    if (appState()?.settings.metadataLookupsEnabled !== true) return;
    if (lookup().kind === "running") return;
    const pass = (lookupPass += 1);
    let hashes: string[];
    try {
      hashes = await unanswered();
    } catch {
      return;
    }
    if (hashes.length === 0) {
      setLookup({ kind: "done" });
      return;
    }
    setLookup({ kind: "running", asked: 0, total: hashes.length });
    const end = await runLookups({
      hashes,
      fetch: (batch) => engine.fetchMetadataBatch(batch, true),
      sleep: (ms) => new Promise((resolve) => setTimeout(resolve, ms)),
      shouldStop: () =>
        disposed ||
        pass !== lookupPass ||
        appState()?.settings.metadataLookupsEnabled !== true,
      onAnswers: (answers, asked) => {
        if (disposed) return;
        const bySha = new Map(answers.map((m) => [m.sha256, m]));
        batch(() => {
          setLibrary((rows) =>
            rows.map((row) => {
              const found = bySha.get(row.sha256);
              return found ? { ...row, metadata: found } : row;
            }),
          );
          setLookup({ kind: "running", asked, total: hashes.length });
        });
      },
    });
    if (disposed || pass !== lookupPass) return;
    if (end.kind === "refused") {
      setLookup({ kind: "unreachable", message: messageOf(end.error) });
    } else {
      setLookup(end.kind === "done" ? { kind: "done" } : { kind: "idle" });
    }
  };

  const run = async (what: () => Promise<unknown>, onOk?: string) => {
    try {
      await what();
      await refresh();
      if (onOk) showToast(onOk);
      return true;
    } catch (error) {
      showToast(messageOf(error), "bad");
      return false;
    }
  };

  // ── the engine drives these ───────────────────────────────────────────────

  const stops = [
    engine.onScanProgress((p) => {
      batch(() => {
        setScanProgress(p);
        if (freshInstalls().length > 0) setFreshInstalls([]);
      });
    }),
    engine.onScanDone((result) => {
      batch(() => {
        // The first record lands with the end of the progress, so setup
        // never shows again in the moment before the refresh reads it back.
        if (scan() === null) setScan(result);
        setScanProgress(null);
        setUnticked(new Set<string>());
        setShowAllDuplicates(false);
      });
      if (result.cancelled) showToast("Scan cancelled · nothing was changed");
      else lookupDue = true;
      void refresh();
    }),
    engine.onScanError((error) => {
      setScanProgress(null);
      showToast(error.message, "bad");
      // The engine is free again, and the screens must know it.
      void refresh();
    }),
    engine.onApplyProgress((p) => setApplyProgress(p)),
    engine.onDownloadProgress((record) => {
      const before = dl.downloads().find((r) => r.downloadId === record.downloadId);
      dl.receive(record);
      // A finished download added a vault file and its links.
      if (
        (record.state === "done" || record.state === "linkedOnly") &&
        before?.state !== record.state
      ) {
        void refresh();
      }
    }),
    engine.onApplyDone((result) => {
      batch(() => {
        setApplyProgress(null);
        setLastApply(result);
      });
      void refresh();
    }),
    engine.onApplyError((error) => {
      setApplyProgress(null);
      showToast(error.message, "bad");
    }),
    engine.onRevertProgress((p) => setRevertProgress(p)),
    engine.onRevertDone(() => {
      batch(() => {
        setRevertProgress(null);
        setLastApply(null);
      });
      showToast("Run undone · every file is back where it was");
      void refresh();
    }),
    engine.onRevertError((error) => {
      setRevertProgress(null);
      // A stop the person asked for is not a failure. The Stop button already
      // said it is stopping, and the screen that follows says where it got to.
      if (error.code !== "cancelled") showToast(error.message, "bad");
      // Steps already undone stay undone, so what is on disk has changed.
      void refresh();
    }),
  ];
  onCleanup(() => {
    disposed = true;
    for (const stop of stops) stop();
    if (toastTimer) clearTimeout(toastTimer);
  });

  void refresh();

  const actions: Actions = {
    go(next) {
      batch(() => {
        if (next !== screen()) setNameResult(null);
        setScreen(next);
        setCategoryMenuOpen(false);
      });
    },
    showToast,
    refresh,
    run,
    toggleGroup(groupId) {
      setUnticked((current: ReadonlySet<string>): ReadonlySet<string> => {
        const next = new Set<string>(current);
        if (next.has(groupId)) next.delete(groupId);
        else next.add(groupId);
        return next;
      });
    },
    setShowAllDuplicates,
    setShowSingles,
    setCategoryMenuOpen,
  };

  return {
    engine,
    appState,
    vault,
    installs,
    scan,
    plan,
    vaultFiles,
    nameGroups,
    nameCards,
    hiddenNameCards,
    nameResult,
    setNameResult,
    orphans,
    health,
    danglingLinks,
    running,
    interrupted,
    cutOffRun,
    lastApply,
    runOnScreen,
    appliedPlan,
    scanPredatesRun,
    scanPredatesUndo,
    scanPredatesSetAside,
    usage,
    usageMethod,
    lookup,
    scanProgress,
    applyProgress,
    revertProgress,
    linkBusy,
    anyBusy,
    ready,
    failure,
    planView,
    library,
    contents,
    libraryTotal,
    nothingSearched,
    installViews,
    selection,
    gate,
    hasInstalls,
    hasVault,
    vaultVolume,
    drives,
    setupDone,
    missingStep,
    freshInstalls,
    setFreshInstalls,
    nothingRead,
    unusedCount,
    screen,
    toast,
    unticked,
    showAllDuplicates,
    showSingles,
    categoryMenuOpen,
    lib,
    setLib,
    dl,
    modal: () => modal.current,
    setModal: (next) => setModalStore("current", next),
    patchModal: (fn) =>
      setModalStore(
        "current",
        produce((current) => {
          if (current) fn(current);
        }),
      ),
    actions,
  };
}
