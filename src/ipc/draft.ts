/**
 * TEMPORARY: the downloader types as agreed with the engine side, until they
 * land in contract.ts. Merged into the contract module so the code can use its
 * final import paths.
 */

export type TokenService = "huggingface" | "civitai";

/** Never carries the token itself. */
export interface TokenStatus {
  saved: boolean;
  /** Null when the site could not be asked. */
  ok: boolean | null;
  /** The Hugging Face account name. Null for Civitai. */
  account: string | null;
  /** The site's words when ok is false, or why it could not be asked. */
  message: string | null;
}

export type DownloadHost = "huggingface" | "civitai";

/** A Hugging Face repository, for "Open the model's page". */
export interface HfPage {
  owner: string;
  repo: string;
}

export type AddressRefusalKind =
  | "badAddress"
  | "hfRepoNotFile"
  | "tokenMissing"
  | "tokenRejected"
  | "noAccess"
  | "notFound";

export interface AddressRefusal {
  kind: AddressRefusalKind;
  /** Null only for badAddress. */
  host: DownloadHost | null;
  /** What the site calls the model, when it said. */
  title: string | null;
  subtitle: string | null;
  /** The site's own words, exactly as it sent them. */
  serviceMessage: string | null;
  /** Hugging Face only. */
  page: HfPage | null;
}

export type InstallLinkState = "free" | "hasLink" | "nameTaken" | "unavailable";

export interface AddressPlan {
  host: DownloadHost;
  /** The Civitai model name, or the Hugging Face file name. */
  title: string;
  /** The Hugging Face repo, or the address as read. */
  subtitle: string;
  /** Civitai only, newest first. */
  versions: { id: number; name: string }[];
  versionId: number | null;
  /** Civitai only, primary first. */
  files: { id: number; name: string; sizeBytes: number; detail: string }[];
  /** Civitai only: the file this plan is for. */
  fileId: number | null;
  fileName: string;
  sizeBytes: number;
  /** Null for a small Hugging Face file stored without LFS. */
  sha256: string | null;
  /** The categories for the folder menu. */
  categories: string[];
  /** The folder this plan was worked out for, or null when none is known. */
  category: string | null;
  suggestedCategory: string | null;
  /** For example "Civitai calls it a Checkpoint". */
  suggestedBecause: string | null;
  alreadyInVault: { vaultRelPath: string } | null;
  /** With the __XXXXXXXX tag when the name is taken. Null without a folder. */
  vaultRelPath: string | null;
  /** Another model already has this name in the vault, so the name has a tag. */
  vaultNameTaken: boolean;
  installs: {
    installId: string;
    /** Null without a folder, and for an install that cannot be reached. */
    linkPath: string | null;
    state: InstallLinkState;
    /** Ticked on a new card: the installs ticked last time, or every one. */
    ticked: boolean;
  }[];
  vaultFreeBytes: number | null;
  /** The file, less any part already kept, plus the margin the engine keeps free. */
  spaceNeededBytes: number;
  /** Hugging Face only. */
  page: HfPage | null;
  /** Civitai only. */
  modelId: number | null;
}

export type AddressReading =
  | { plan: AddressPlan; refusal: null }
  | { plan: null; refusal: AddressRefusal };

export type DownloadState =
  | "waiting"
  | "running"
  | "checking"
  | "stopped"
  | "failed"
  | "mismatch"
  | "cutOff"
  | "done"
  | "linkedOnly";

export type DownloadErrorKind =
  | "connection"
  | "refused"
  | "expired"
  | "noSpace"
  | "mismatch"
  | "disk"
  | "changedOnSite";

export interface DownloadError {
  kind: DownloadErrorKind;
  /** One sentence for the person, in the engine's words. */
  message: string;
  /** The site's own words, when it said something. */
  serviceMessage: string | null;
}

export interface Download {
  downloadId: string;
  host: DownloadHost;
  title: string;
  fileName: string;
  category: string;
  /** Where it goes, or went. */
  vaultRelPath: string;
  /** The installs to link. */
  installIds: string[];
  /** Filled once done or linkedOnly. */
  linkedInstallIds: string[];
  /** An install that could not get its link at the end, and why. */
  notLinked: { installId: string; reason: string }[];
  /** Nothing new went into the vault: it held the file already. */
  alreadyInVault: boolean;
  /** The file's SHA-256, null until it is known. */
  sha256: string | null;
  state: DownloadState;
  bytesDone: number;
  bytesTotal: number;
  /** An average over the last 5 seconds. Null before it is known. */
  bytesPerSecond: number | null;
  error: DownloadError | null;
  startedAt: string | null;
  finishedAt: string | null;
}

declare module "~/ipc/contract" {
  interface Engine {
    setToken(service: TokenService, token: string): Promise<{ ok: true; account: string | null }>;
    getTokenStatus(service: TokenService): Promise<TokenStatus>;
    removeToken(service: TokenService): Promise<{ removed: true }>;
    readModelAddress(args: {
      address: string;
      versionId?: number;
      fileId?: number;
      category?: string;
    }): Promise<AddressReading>;
    openHuggingFacePage(owner: string, repo: string): Promise<null>;
    startDownload(args: {
      address: string;
      versionId?: number;
      fileId?: number;
      category: string;
      installIds: string[];
    }): Promise<Download>;
    stopDownload(downloadId: string): Promise<Download>;
    continueDownload(downloadId: string): Promise<Download>;
    discardDownload(downloadId: string): Promise<{ removed: true }>;
    removeDownload(downloadId: string): Promise<{ removed: true }>;
    listDownloads(): Promise<Download[]>;
    onDownloadProgress(fn: (download: Download) => void): Unsubscribe;
  }
}
