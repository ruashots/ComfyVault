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
import { isVaultError, nothingWasSearched } from "~/ipc/contract";
import type {
  ApplyProgress,
  ApplyRecord,
  AppState,
  ConsolidationPlan,
  ContentRow,
  DirectoryEntry,
  Engine,
  Install,
  InstallCandidate,
  InterruptedApply,
  LinkRecord,
  NameGroup,
  RunningComfy,
  ScanProgress,
  ScanRecord,
  UsageResult,
  VaultFile,
  VaultHealth,
  VaultInfo,
} from "~/ipc/contract";

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

export type PickerPurpose = "install" | "vault" | "link";

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
  /** Which content a link is being placed for. */
  sha256: string | null;
  /** Which install is being re-pointed, when the person pressed Edit. */
  replacing: string | null;
  /**
   * Why the last press of the button did not happen. It stays in front of the
   * person until they act, because a toast can be missed and an answer to
   * something they just pressed is not allowed to go by unnoticed.
   */
  error: string | null;
}

export type ConfirmLine = ReadonlyArray<{ text: string; emph?: boolean }>;

export interface ConfirmModal {
  kind: "confirm";
  title: string;
  body: ConfirmLine[];
  cta: string;
  action: () => Promise<void> | void;
  running: boolean;
  error: string | null;
  /** What the engine named as being in the way. One entry per line. */
  errorDetail: readonly string[];
}

export type Modal = PickerModal | ConfirmModal;

export interface AppStore {
  readonly engine: Engine;

  readonly appState: Accessor<AppState | null>;
  readonly vault: Accessor<VaultInfo | null>;
  readonly installs: Accessor<readonly Install[]>;
  readonly scan: Accessor<ScanRecord | null>;
  readonly plan: Accessor<ConsolidationPlan | null>;
  readonly vaultFiles: Accessor<readonly VaultFile[]>;
  readonly nameGroups: Accessor<readonly NameGroup[]>;
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
  readonly lastApply: Accessor<ApplyRecord | null>;
  readonly usage: Accessor<ReadonlyMap<string, UsageResult>>;
  readonly usageMethod: Accessor<string | null>;

  readonly scanProgress: Accessor<ScanProgress | null>;
  readonly applyProgress: Accessor<ApplyProgress | null>;
  readonly ready: Accessor<boolean>;
  readonly failure: Accessor<string | null>;

  readonly planView: Accessor<PlanView | null>;
  readonly library: Accessor<readonly ContentRow[]>;
  /** How many contents the engine holds in total, beyond the page loaded. */
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
   * An install folder the person has chosen while there was still no vault to
   * record it in. The engine refuses to register one before a vault exists, so
   * the interface holds it until the vault folder is chosen and then registers
   * it. It lives only in this window until then.
   */
  readonly pendingInstall: Accessor<string | null>;
  readonly setPendingInstall: (path: string | null) => void;
  /** Both things a person has to set before any screen has anything to show. */
  readonly setupDone: Accessor<boolean>;
  /**
   * What is still to be set, said as the thing it is. Null once both are done.
   */
  readonly missingStep: Accessor<string | null>;
  readonly unusedCount: Accessor<number>;

  readonly screen: Accessor<Screen>;
  readonly toast: Accessor<Toast | null>;
  readonly unticked: Accessor<ReadonlySet<string>>;
  readonly showAllDuplicates: Accessor<boolean>;
  readonly showSingles: Accessor<boolean>;
  readonly categoryMenuOpen: Accessor<boolean>;
  readonly renaming: Accessor<{ sha256: string; value: string } | null>;

  readonly lib: LibraryView;
  readonly setLib: SetStoreFunction<LibraryView>;
  readonly modal: Accessor<Modal | null>;
  readonly setModal: (modal: Modal | null) => void;
  readonly patchModal: (fn: (modal: Modal) => void) => void;

  readonly actions: Actions;
}

export interface Actions {
  go(screen: Screen): void;
  showToast(message: string, tone?: "ok" | "bad"): void;
  refresh(): Promise<void>;
  /** Run an engine call and turn any refusal into a message the person can read. */
  run(what: () => Promise<unknown>, onOk?: string): Promise<boolean>;
  toggleGroup(groupId: string): void;
  setShowAllDuplicates(on: boolean): void;
  setShowSingles(on: boolean): void;
  setCategoryMenuOpen(on: boolean): void;
  startRename(sha256: string, value: string): void;
  setRenameValue(value: string): void;
  cancelRename(): void;
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

export function createAppStore(engine: Engine): AppStore {
  const [appState, setAppState] = createSignal<AppState | null>(null);
  const [vault, setVault] = createSignal<VaultInfo | null>(null);
  const [installs, setInstalls] = createSignal<readonly Install[]>([]);
  const [scan, setScan] = createSignal<ScanRecord | null>(null);
  const [plan, setPlan] = createSignal<ConsolidationPlan | null>(null);
  const [vaultFiles, setVaultFiles] = createSignal<readonly VaultFile[]>([]);
  const [nameGroups, setNameGroups] = createSignal<readonly NameGroup[]>([]);
  const [orphans, setOrphans] = createSignal<readonly VaultFile[]>([]);
  const [health, setHealth] = createSignal<VaultHealth | null>(null);
  const [running, setRunning] = createSignal<readonly RunningComfy[]>([]);
  const [interrupted, setInterrupted] = createSignal<readonly InterruptedApply[]>([]);
  const [lastApply, setLastApply] = createSignal<ApplyRecord | null>(null);
  const [usage, setUsage] = createSignal<ReadonlyMap<string, UsageResult>>(new Map());
  const [library, setLibrary] = createSignal<readonly ContentRow[]>([]);
  const [libraryTotal, setLibraryTotal] = createSignal(0);
  const [nothingSearched, setNothingSearched] = createSignal(false);

  const [scanProgress, setScanProgress] = createSignal<ScanProgress | null>(null);
  const [applyProgress, setApplyProgress] = createSignal<ApplyProgress | null>(null);
  const [ready, setReady] = createSignal(false);
  const [failure, setFailure] = createSignal<string | null>(null);

  const [screen, setScreen] = createSignal<Screen>("home");
  const [toast, setToast] = createSignal<Toast | null>(null);
  const [unticked, setUnticked] = createSignal<ReadonlySet<string>>(new Set<string>());
  const [showAllDuplicates, setShowAllDuplicates] = createSignal(false);
  const [showSingles, setShowSingles] = createSignal(false);
  const [categoryMenuOpen, setCategoryMenuOpen] = createSignal(false);
  const [renaming, setRenaming] = createSignal<{ sha256: string; value: string } | null>(
    null,
  );
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

  let toastTimer: ReturnType<typeof setTimeout> | null = null;
  const showToast = (message: string, tone: "ok" | "bad" = "ok") => {
    if (toastTimer) clearTimeout(toastTimer);
    setToast({ message, tone });
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

  const [pendingInstall, setPendingInstall] = createSignal<string | null>(null);
  const hasInstalls = createMemo(
    () => installs().length > 0 || pendingInstall() !== null,
  );
  const hasVault = createMemo(() => appState()?.vaultInitialized === true);
  const setupDone = createMemo(() => hasInstalls() && hasVault());
  const missingStep = createMemo(() => {
    if (setupDone()) return null;
    if (!hasInstalls() && !hasVault()) {
      return "Register a ComfyUI install and choose where the vault goes.";
    }
    if (!hasInstalls()) {
      return "Register a ComfyUI install. The vault folder is already set.";
    }
    // "set" rather than "registered": an install chosen before there is a
    // vault is held in the window until one exists, so it is not registered
    // anywhere yet and saying so would be untrue.
    return "Choose where the vault goes. Your ComfyUI installs are set.";
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
    return library().filter((row) => answers.get(row.name)?.used === false).length;
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
          setOrphans([]);
          setHealth(null);
          setLibrary([]);
          setLibraryTotal(0);
          setRunning([]);
          setInterrupted([]);
          setLastApply(null);
          setUsage(new Map());
          setNothingSearched(false);
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

      const nextPlan =
        lastScan && !lastScan.cancelled
          ? await orNotYet(engine.buildPlan(lastScan.scanId), null)
          : null;

      const [files, groups, orphanList, vaultHealth, contents] = await Promise.all([
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
      ]);

      batch(() => {
        setAppState(state);
        setVault(vaultInfo);
        setInstalls(installList);
        setScan(lastScan);
        setPlan(nextPlan);
        setVaultFiles(files.files);
        setNameGroups(groups);
        setOrphans(orphanList);
        setHealth(vaultHealth);
        setLibrary(contents.rows);
        setLibraryTotal(contents.total);
        setRunning(runningList);
        setInterrupted(interruptedList);
        setLastApply(applies.find((a) => a.state !== "reverted") ?? null);
        setFailure(null);
        setReady(true);
      });

      await loadUsage(contents.rows);
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
    engine.onScanProgress((p) => setScanProgress(p)),
    engine.onScanDone((result) => {
      batch(() => {
        setScanProgress(null);
        setUnticked(new Set<string>());
        setShowAllDuplicates(false);
      });
      if (result.cancelled) showToast("Scan cancelled · nothing was changed");
      void refresh();
    }),
    engine.onScanError((error) => {
      setScanProgress(null);
      showToast(error.message, "bad");
    }),
    engine.onApplyProgress((p) => setApplyProgress(p)),
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
    engine.onRevertProgress((p) => setApplyProgress(p)),
    engine.onRevertDone(() => {
      batch(() => {
        setApplyProgress(null);
        setLastApply(null);
      });
      showToast("Run undone · every file is back where it was");
      void refresh();
    }),
    engine.onRevertError((error) => showToast(error.message, "bad")),
  ];
  onCleanup(() => {
    for (const stop of stops) stop();
    if (toastTimer) clearTimeout(toastTimer);
  });

  void refresh();

  const actions: Actions = {
    go(next) {
      batch(() => {
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
    startRename(sha256, value) {
      setRenaming({ sha256, value });
    },
    setRenameValue(value) {
      setRenaming((current) => (current ? { ...current, value } : null));
    },
    cancelRename() {
      setRenaming(null);
    },
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
    orphans,
    health,
    danglingLinks,
    running,
    interrupted,
    lastApply,
    usage,
    usageMethod,
    scanProgress,
    applyProgress,
    ready,
    failure,
    planView,
    library,
    libraryTotal,
    nothingSearched,
    installViews,
    selection,
    gate,
    hasInstalls,
    hasVault,
    pendingInstall,
    setPendingInstall,
    setupDone,
    missingStep,
    unusedCount,
    screen,
    toast,
    unticked,
    showAllDuplicates,
    showSingles,
    categoryMenuOpen,
    renaming,
    lib,
    setLib,
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
