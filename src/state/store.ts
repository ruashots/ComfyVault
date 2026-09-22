/**
 * The one place the interface keeps what it knows.
 *
 * Everything the screens print is either raw engine data held here, or a memo
 * derived from it. Nothing is stored twice: the plan, the selection totals and
 * the Apply gate are all computed from the scan, the machine and the set of
 * models the person unticked.
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

import { derivePlan, type Plan } from "~/domain/plan";
import {
  applyGate,
  selectionFor,
  EMPTY_SELECTION,
  type ApplyGate,
  type Selection,
} from "~/domain/selection";
import type {
  ApplyProgress,
  ApplyResult,
  Engine,
  FolderCheck,
  MachineState,
  PickerPurpose,
  ScanProgress,
  ScanResult,
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
  folder: string;
  unusedOnly: boolean;
  sort: LibrarySort;
  selected: string | null;
  drawerOpen: boolean;
}

export interface Toast {
  message: string;
  tone: "ok" | "bad";
}

export interface TreeNode {
  path: string;
  name: string;
  kind: "drive" | "folder";
  depth: number;
  looksLikeInstall: boolean | null;
  readable: boolean;
  /** The person created it in this picker a moment ago. */
  isNew: boolean;
  /** Null until the engine has been asked what is inside. */
  hasChildren: boolean | null;
}

export interface NewFolderDraft {
  parent: string;
  name: string;
  error: string | null;
  saving: boolean;
}

export interface PickerModal {
  kind: "picker";
  purpose: PickerPurpose;
  nodes: TreeNode[];
  expanded: string[];
  loading: string[];
  picked: string | null;
  check: FolderCheck | null;
  checking: boolean;
  newFolder: NewFolderDraft | null;
  /** Which model a link is being placed for, when the purpose is "link". */
  modelId: string | null;
  /** Which install is being re-pointed, when the person pressed Edit. */
  replacing: string | null;
}

export interface ConfirmModal {
  kind: "confirm";
  title: string;
  /** Each entry is a paragraph. Parts marked emphasised are the nouns at stake. */
  body: ConfirmLine[];
  cta: string;
  action: () => Promise<void> | void;
  running: boolean;
}

export type ConfirmLine = ReadonlyArray<{ text: string; emph?: boolean }>;

export type Modal = PickerModal | ConfirmModal;

export interface AppState {
  readonly engine: Engine;

  readonly scan: Accessor<ScanResult | null>;
  readonly machine: Accessor<MachineState | null>;
  readonly screen: Accessor<Screen>;
  readonly scanProgress: Accessor<ScanProgress | null>;
  readonly applyProgress: Accessor<ApplyProgress | null>;
  readonly lastRun: Accessor<ApplyResult | null>;
  readonly toast: Accessor<Toast | null>;
  readonly ready: Accessor<boolean>;

  readonly plan: Accessor<Plan | null>;
  readonly selection: Accessor<Selection>;
  readonly gate: Accessor<ApplyGate>;
  readonly hasInstances: Accessor<boolean>;
  /** What the run would return if every running ComfyUI were closed first. */
  readonly reclaimIfClosed: Accessor<number>;

  readonly unticked: Accessor<ReadonlySet<string>>;
  readonly showAllDuplicates: Accessor<boolean>;
  readonly showSingles: Accessor<boolean>;
  readonly folderMenuOpen: Accessor<boolean>;
  readonly renaming: Accessor<{ modelId: string; value: string } | null>;

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
  dismissToast(): void;
  refresh(): Promise<void>;
  toggleModel(id: string): void;
  setShowAllDuplicates(on: boolean): void;
  setShowSingles(on: boolean): void;
  setFolderMenuOpen(on: boolean): void;
  startRename(modelId: string, value: string): void;
  setRenameValue(value: string): void;
  cancelRename(): void;
}

const AppContext = createContext<AppState>();

export function useApp(): AppState {
  const app = useContext(AppContext);
  if (!app) throw new Error("useApp was called outside the app");
  return app;
}

export const AppProvider = AppContext.Provider;

export function createAppState(engine: Engine): AppState {
  const [scan, setScan] = createSignal<ScanResult | null>(null);
  const [machine, setMachine] = createSignal<MachineState | null>(null);
  const [screen, setScreen] = createSignal<Screen>("home");
  const [scanProgress, setScanProgress] = createSignal<ScanProgress | null>(null);
  const [applyProgress, setApplyProgress] = createSignal<ApplyProgress | null>(null);
  const [lastRun, setLastRun] = createSignal<ApplyResult | null>(null);
  const [toast, setToast] = createSignal<Toast | null>(null);
  const [ready, setReady] = createSignal(false);
  const [unticked, setUnticked] = createSignal<ReadonlySet<string>>(new Set<string>());
  const [showAllDuplicates, setShowAllDuplicates] = createSignal(false);
  const [showSingles, setShowSingles] = createSignal(false);
  const [folderMenuOpen, setFolderMenuOpen] = createSignal(false);
  const [renaming, setRenaming] = createSignal<{ modelId: string; value: string } | null>(null);
  const [modal, setModalSignal] = createStore<{ current: Modal | null }>({ current: null });

  const [lib, setLib] = createStore<LibraryView>({
    query: "",
    folder: "all",
    unusedOnly: false,
    sort: "size",
    selected: null,
    drawerOpen: false,
  });

  let toastTimer: ReturnType<typeof setTimeout> | null = null;
  const showToast = (message: string, tone: "ok" | "bad" = "ok") => {
    if (toastTimer) clearTimeout(toastTimer);
    setToast({ message, tone });
    toastTimer = setTimeout(() => setToast(null), 3400);
  };
  const dismissToast = () => {
    if (toastTimer) clearTimeout(toastTimer);
    setToast(null);
  };

  const plan = createMemo<Plan | null>(() => {
    const s = scan();
    const m = machine();
    if (!s || !m) return null;
    return derivePlan(s, m);
  });

  const reclaimIfClosed = createMemo(() => {
    const s = scan();
    const m = machine();
    if (!s || !m) return 0;
    if (m.running.length === 0) return plan()?.totals.reclaimBytes ?? 0;
    return derivePlan(s, m, { ignoreOpenFiles: true }).totals.reclaimBytes;
  });

  const selection = createMemo<Selection>(() => {
    const p = plan();
    if (!p) return EMPTY_SELECTION;
    return selectionFor(p, unticked());
  });

  const gate = createMemo<ApplyGate>(() => {
    const m = machine();
    if (!m) return { can: false, reason: "nothing_ticked" };
    return applyGate(m, selection());
  });

  const hasInstances = createMemo(() => (scan()?.instances.length ?? 0) > 0);

  const refresh = async () => {
    const [nextScan, nextMachine, run] = await Promise.all([
      engine.loadScan(),
      engine.readMachine(),
      engine.lastRun(),
    ]);
    batch(() => {
      setScan(nextScan);
      setMachine(nextMachine);
      setLastRun(run);
      setReady(true);
    });
  };

  // The engine drives these. Every screen reads the same progress object.
  const stops = [
    engine.onScanProgress((p) => setScanProgress(p)),
    engine.onScanFinished((result) => {
      batch(() => {
        setScanProgress(null);
        setScan(result);
        setUnticked(new Set<string>());
        setShowAllDuplicates(false);
      });
      void engine.readMachine().then(setMachine);
    }),
    engine.onScanCancelled(() => {
      setScanProgress(null);
      showToast("Scan cancelled · nothing was changed");
    }),
    engine.onApplyProgress((p) => setApplyProgress(p)),
    engine.onApplyFinished((result) => {
      batch(() => {
        setApplyProgress(null);
        setLastRun(result);
      });
      void refresh();
    }),
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
        setFolderMenuOpen(false);
      });
    },
    showToast,
    dismissToast,
    refresh,
    toggleModel(id) {
      setUnticked((current: ReadonlySet<string>): ReadonlySet<string> => {
        const next = new Set<string>(current);
        if (next.has(id)) next.delete(id);
        else next.add(id);
        return next;
      });
    },
    setShowAllDuplicates,
    setShowSingles,
    setFolderMenuOpen,
    startRename(modelId, value) {
      setRenaming({ modelId, value });
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
    scan,
    machine,
    screen,
    scanProgress,
    applyProgress,
    lastRun,
    toast,
    ready,
    plan,
    selection,
    gate,
    hasInstances,
    reclaimIfClosed,
    unticked,
    showAllDuplicates,
    showSingles,
    folderMenuOpen,
    renaming,
    lib,
    setLib,
    modal: () => modal.current,
    setModal: (next) => setModalSignal("current", next),
    patchModal: (fn) =>
      setModalSignal(
        "current",
        produce((current) => {
          if (current) fn(current);
        }),
      ),
    actions,
  };
}
