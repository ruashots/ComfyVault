/**
 * The engine inside the desktop window.
 *
 * One method per command in docs/IPC-CONTRACT.md. Every command takes exactly
 * one argument object named `args`, and a command with no input takes no
 * argument, which is why `call` and `callNoArgs` are separate.
 *
 * Opening a folder in Explorer and opening a Windows settings page are not
 * engine commands. They come from Tauri's opener plugin, which the Rust app
 * must register with the permissions `opener:allow-open-url` and
 * `opener:allow-reveal-item-in-dir`. That is the only capability the window
 * needs of its own: browsing folders goes through the engine.
 */

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { openUrl, revealItemInDir } from "@tauri-apps/plugin-opener";

import type {
  ApplyProgress,
  ApplyResult,
  AppState,
  ConsolidationPlan,
  DirectoryListing,
  Engine,
  Install,
  InstallCandidate,
  InterruptedApply,
  Link,
  LockState,
  ModelDirNode,
  ModelMetadata,
  NameGroup,
  PlatformReport,
  RunningComfy,
  ScanEntryPage,
  ScanProgress,
  ScanResult,
  Settings,
  Unsubscribe,
  UsageResult,
  VaultError,
  VaultFile,
  VaultFilePage,
  VaultHealth,
  VaultInfo,
} from "~/ipc/contract";

function call<T>(command: string, args: Record<string, unknown>): Promise<T> {
  return invoke<T>(command, { args });
}

function callNoArgs<T>(command: string): Promise<T> {
  return invoke<T>(command);
}

/**
 * Tauri's listen() resolves once the listener is registered. The interface
 * wants to unsubscribe straight away, so the returned function waits for the
 * registration and then removes it. Unsubscribing early still works.
 */
function subscribe<T>(event: string, fn: (payload: T) => void): Unsubscribe {
  let cancelled = false;
  const pending = listen<T>(event, (message) => {
    if (!cancelled) fn(message.payload);
  });
  return () => {
    cancelled = true;
    void pending.then((unlisten) => unlisten()).catch(() => undefined);
  };
}

export function createTauriEngine(): Engine {
  const appWindow = getCurrentWindow();

  return {
    getPlatformReport: () => callNoArgs<PlatformReport>("get_platform_report"),
    getAppState: () => callNoArgs<AppState>("get_app_state"),
    selectVault: (path, createIfMissing) =>
      call<VaultInfo>("select_vault", { path, createIfMissing }),
    getSettings: () => callNoArgs<Settings>("get_settings"),
    updateSettings: (patch) => call<Settings>("update_settings", { ...patch }),

    validateInstallPath: (path) =>
      call<InstallCandidate>("validate_install_path", { path }),
    registerInstall: (path, label) =>
      call<Install>("register_install", label === undefined ? { path } : { path, label }),
    listInstalls: () => callNoArgs<Install[]>("list_installs"),
    refreshInstall: (id) => call<Install>("refresh_install", { id }),
    updateInstall: (id, label) => call<Install>("update_install", { id, label }),
    unregisterInstall: (id) =>
      call<{ removed: true; linksLeftInPlace: number }>("unregister_install", { id }),
    listInstallModelDirs: (id) =>
      call<ModelDirNode[]>("list_install_model_dirs", { id }),

    listDirectory: (path) =>
      call<DirectoryListing>("list_directory", path === null ? {} : { path }),
    createDirectory: (path) =>
      call<{ path: string; created: boolean }>("create_directory", { path }),

    startScan: (installIds) =>
      call<{ scanId: string }>("start_scan", installIds ? { installIds } : {}),
    cancelScan: (scanId) => call<{ cancelled: true }>("cancel_scan", { scanId }),
    getLastScan: () => callNoArgs<ScanResult | null>("get_last_scan"),
    getScanEntries: (args) => call<ScanEntryPage>("get_scan_entries", { ...args }),
    onScanProgress: (fn) => subscribe<ScanProgress>("scan:progress", fn),
    onScanDone: (fn) => subscribe<ScanResult>("scan:done", fn),
    onScanError: (fn) => subscribe<VaultError>("scan:error", fn),

    buildPlan: (scanId) => call<ConsolidationPlan>("build_plan", { scanId }),
    getPlan: (planId) => call<ConsolidationPlan>("get_plan", { planId }),

    startApply: (args) => call<{ applyId: string }>("start_apply", { ...args }),
    cancelApply: (applyId) =>
      call<{ cancelled: true }>("cancel_apply", { applyId }),
    getApplyResult: (applyId) =>
      call<ApplyResult>("get_apply_result", { applyId }),
    listApplies: () => callNoArgs<ApplyResult[]>("list_applies"),
    getInterruptedApplies: () =>
      callNoArgs<InterruptedApply[]>("get_interrupted_applies"),
    resumeApply: (applyId) => call<{ applyId: string }>("resume_apply", { applyId }),
    revertApply: (applyId) => call<{ applyId: string }>("revert_apply", { applyId }),
    onApplyProgress: (fn) => subscribe<ApplyProgress>("apply:progress", fn),
    onApplyDone: (fn) => subscribe<ApplyResult>("apply:done", fn),
    onApplyError: (fn) => subscribe<VaultError>("apply:error", fn),
    onRevertProgress: (fn) => subscribe<ApplyProgress>("revert:progress", fn),
    onRevertDone: (fn) => subscribe<ApplyResult>("revert:done", fn),
    onRevertError: (fn) => subscribe<VaultError>("revert:error", fn),

    createLink: (args) => call<Link>("create_link", { createDir: false, ...args }),
    removeLink: (linkId) => call<{ removed: true }>("remove_link", { linkId }),
    createModelFolder: (installId, relativeDir) =>
      call<{ absPath: string; created: boolean }>("create_model_folder", {
        installId,
        relativeDir,
      }),
    listLinks: (filter) => call<Link[]>("list_links", { ...(filter ?? {}) }),

    listVaultFiles: (args) => call<VaultFilePage>("list_vault_files", { ...args }),
    listNameGroups: () => callNoArgs<NameGroup[]>("list_name_groups"),
    setCanonicalName: (sha256, name) =>
      call<VaultFile>("set_canonical_name", { sha256, name }),
    removeAlias: (sha256, name) =>
      call<{ removed: true }>("remove_alias", { sha256, name }),
    listOrphans: () => callNoArgs<VaultFile[]>("list_orphans"),
    deleteVaultFile: (sha256, confirm) =>
      call<{ deleted: true; bytesFreed: number }>("delete_vault_file", {
        sha256,
        confirm,
      }),
    checkVaultHealth: () => callNoArgs<VaultHealth>("check_vault_health"),

    checkModelUsage: (names, installIds) =>
      call<UsageResult[]>("check_model_usage", installIds ? { names, installIds } : { names }),
    getMetadata: (sha256, refresh) =>
      call<ModelMetadata | null>("get_metadata", { sha256, refresh }),
    fetchMetadataBatch: (sha256, refresh) =>
      call<ModelMetadata[]>("fetch_metadata_batch", { sha256, refresh }),

    getRunningComfy: () => callNoArgs<RunningComfy[]>("get_running_comfy"),
    checkLockedFiles: (paths) => call<LockState[]>("check_locked_files", { paths }),

    openExternal: (target) => openUrl(target),
    revealInFileManager: (path) => revealItemInDir(path),
    windowMinimize: () => appWindow.minimize(),
    windowToggleMaximize: () => appWindow.toggleMaximize(),
    windowClose: () => appWindow.close(),
  };
}
