/**
 * The engine inside the desktop window.
 *
 * Every command name and every event name lives in the two tables below and
 * nowhere else, so aligning with docs/IPC-CONTRACT.md is an edit in one place.
 * comfyvault-core owns those names; the payload shapes are the types in
 * src/ipc/contract.ts.
 */

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

import type {
  ApplyProgress,
  ApplyResult,
  CreateFolderResult,
  Engine,
  FolderCheck,
  FolderEntry,
  InstanceId,
  MachineState,
  PickerPurpose,
  ScanProgress,
  ScanResult,
  Unsubscribe,
} from "~/ipc/contract";

/** The Rust commands this interface calls. */
const CMD = {
  loadScan: "load_scan",
  readMachine: "read_machine",
  startScan: "start_scan",
  cancelScan: "cancel_scan",
  startApply: "start_apply",
  stopApply: "stop_apply",
  lastRun: "last_run",
  revert: "revert_run",
  listFolder: "list_folder",
  checkFolder: "check_folder",
  createFolder: "create_folder",
  addInstance: "add_instance",
  setInstancePath: "set_instance_path",
  removeInstance: "remove_instance",
  setVaultPath: "set_vault_path",
  setCivitaiEnabled: "set_civitai_enabled",
  setVaultName: "set_vault_name",
  dropName: "drop_name",
  addLink: "add_link",
  deleteOrphan: "delete_orphan",
  markCopyInstead: "mark_copy_instead",
  openWindowsDeveloperSettings: "open_windows_developer_settings",
  openInExplorer: "open_in_explorer",
} as const;

/** The events the engine emits while it works. */
const EVENT = {
  scanProgress: "scan:progress",
  scanFinished: "scan:finished",
  scanCancelled: "scan:cancelled",
  applyProgress: "apply:progress",
  applyFinished: "apply:finished",
} as const;

/**
 * Tauri's listen() resolves after the listener is registered. The interface
 * wants to unsubscribe straight away, so the returned function waits for the
 * registration and then removes it. Unsubscribing before registration finishes
 * still works.
 */
function subscribe<T>(event: string, fn: (payload: T) => void): Unsubscribe {
  let cancelled = false;
  const pending = listen<T>(event, (message) => fn(message.payload));
  void pending.then((unlisten) => {
    if (cancelled) unlisten();
  });
  return () => {
    cancelled = true;
    void pending.then((unlisten) => unlisten());
  };
}

export function createTauriEngine(): Engine {
  const appWindow = getCurrentWindow();

  return {
    loadScan: () => invoke<ScanResult | null>(CMD.loadScan),
    readMachine: () => invoke<MachineState>(CMD.readMachine),

    startScan: () => invoke<void>(CMD.startScan),
    cancelScan: () => invoke<void>(CMD.cancelScan),
    onScanProgress: (fn) => subscribe<ScanProgress>(EVENT.scanProgress, fn),
    onScanFinished: (fn) => subscribe<ScanResult>(EVENT.scanFinished, fn),
    onScanCancelled: (fn) => subscribe<null>(EVENT.scanCancelled, () => fn()),

    startApply: (modelIds: string[]) =>
      invoke<void>(CMD.startApply, { modelIds }),
    stopApply: () => invoke<void>(CMD.stopApply),
    onApplyProgress: (fn) => subscribe<ApplyProgress>(EVENT.applyProgress, fn),
    onApplyFinished: (fn) => subscribe<ApplyResult>(EVENT.applyFinished, fn),
    lastRun: () => invoke<ApplyResult | null>(CMD.lastRun),
    revert: (runId: string) => invoke<void>(CMD.revert, { runId }),

    listFolder: (path: string | null, purpose: PickerPurpose) =>
      invoke<FolderEntry[]>(CMD.listFolder, { path, purpose }),
    checkFolder: (path: string, purpose: PickerPurpose) =>
      invoke<FolderCheck>(CMD.checkFolder, { path, purpose }),
    createFolder: (parent: string, name: string) =>
      invoke<CreateFolderResult>(CMD.createFolder, { parent, name }),

    addInstance: (path: string) => invoke<ScanResult>(CMD.addInstance, { path }),
    setInstancePath: (id: InstanceId, path: string) =>
      invoke<ScanResult>(CMD.setInstancePath, { id, path }),
    removeInstance: (id: InstanceId) =>
      invoke<ScanResult>(CMD.removeInstance, { id }),
    setVaultPath: (path: string) =>
      invoke<MachineState>(CMD.setVaultPath, { path }),
    setCivitaiEnabled: (on: boolean) =>
      invoke<void>(CMD.setCivitaiEnabled, { on }),

    setVaultName: (modelId: string, name: string) =>
      invoke<ScanResult>(CMD.setVaultName, { modelId, name }),
    dropName: (modelId: string, name: string) =>
      invoke<ScanResult>(CMD.dropName, { modelId, name }),
    addLink: (modelId: string, folder: string) =>
      invoke<ScanResult>(CMD.addLink, { modelId, folder }),
    deleteOrphan: (modelId: string) =>
      invoke<ScanResult>(CMD.deleteOrphan, { modelId }),
    markCopyInstead: (modelId: string, placementId: string) =>
      invoke<ScanResult>(CMD.markCopyInstead, { modelId, placementId }),

    openWindowsDeveloperSettings: () =>
      invoke<void>(CMD.openWindowsDeveloperSettings),
    openInExplorer: (path: string) => invoke<void>(CMD.openInExplorer, { path }),

    windowMinimize: () => appWindow.minimize(),
    windowToggleMaximize: () => appWindow.toggleMaximize(),
    windowClose: () => appWindow.close(),
  };
}
