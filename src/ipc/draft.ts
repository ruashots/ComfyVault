/**
 * TEMPORARY: the downloader types as proposed to the engine side, until they
 * land in contract.ts. Merged into the contract module so the code can use its
 * final import paths.
 */

// ── downloads (draft from cv-ui, for cv-engine to take or change) ──────────

export type TokenService = "huggingface" | "civitai";

/** Never carries the token itself. */
export interface TokenStatus {
  saved: boolean;
  /** Null when the service could not be asked. */
  ok: boolean | null;
  /** The Hugging Face account name. Null for Civitai. */
  account: string | null;
  /** The service's own words when ok is false. */
  message: string | null;
}

export type DownloadHost = "huggingface" | "civitai";

export type AddressRefusalKind =
  | "badAddress"
  | "hfRepoNotFile"
  | "tokenMissing"
  | "tokenRejected"
  | "noAccess"
  | "notFound";

/** A Hugging Face repository, for "Open the model's page". */
export interface HfPage {
  owner: string;
  repo: string;
}

export interface AddressRefusal {
  kind: AddressRefusalKind;
  host: DownloadHost | null;
  /** The site's own words, exactly as it sent them. */
  serviceMessage: string | null;
  page: HfPage | null;
}

export type FailureKind =
  | "connection"
  | "refused"
  | "expired"
  | "noSpace"
  | "mismatch"
  | "disk"
  | "changedOnSite";

export interface DownloadFailure {
  kind: FailureKind;
  /** One sentence for the person, in the engine's words. */
  message: string;
  /** The site's own words, when it said something. */
  serviceMessage: string | null;
}

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
  fileId: number | null;
  fileName: string;
  sizeBytes: number;
  /** Null for a small Hugging Face file stored without LFS. */
  sha256: string | null;
  /** Every vault category, for the folder menu. */
  categories: string[];
  /** The folder in effect: the one asked for, else the suggestion, else null. */
  category: string | null;
  suggestedCategory: string | null;
  /** For example "Civitai calls it a Checkpoint". */
  suggestedBecause: string | null;
  alreadyInVault: { vaultRelPath: string } | null;
  /** With the __XXXXXXXX tag when the name is taken. */
  vaultRelPath: string;
  installs: { installId: string; linkPath: string; state: "free" | "hasLink" | "nameTaken" }[];
  vaultFreeBytes: number | null;
  refusal: AddressRefusal | null;
}

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

export interface DownloadRecord {
  downloadId: string;
  host: DownloadHost;
  address: string;
  versionId: number | null;
  fileId: number | null;
  fileName: string;
  sizeBytes: number;
  bytesDone: number;
  /** An average over the last 5 seconds. Null before it is known. */
  bytesPerSecond: number | null;
  state: DownloadState;
  category: string;
  vaultRelPath: string;
  /** The final SHA-256, null while it is not known. */
  sha256: string | null;
  /** The installs asked for. */
  installIds: string[];
  /** The installs actually linked. */
  linkedInstallIds: string[];
  /** When the vault turned out to hold the file already. */
  alreadyInVault: "before" | "after" | null;
  error: DownloadFailure | null;
  startedAt: string;
  finishedAt: string | null;
}


declare module "~/ipc/contract" {
  interface Settings {
    downloadInstallIds: string[] | null;
  }
  interface Engine {
    setToken(service: TokenService, token: string): Promise<{ ok: true; account: string | null }>;
    getTokenStatus(service: TokenService): Promise<TokenStatus>;
    removeToken(service: TokenService): Promise<{ removed: true }>;
    readModelAddress(args: {
      address: string;
      versionId?: number;
      fileId?: number;
      category?: string;
    }): Promise<AddressPlan>;
    openModelPage(address: string): Promise<null>;
    startDownload(args: {
      address: string;
      versionId?: number;
      fileId?: number;
      category: string;
      installIds: string[];
    }): Promise<DownloadRecord>;
    stopDownload(downloadId: string): Promise<DownloadRecord>;
    continueDownload(downloadId: string): Promise<DownloadRecord>;
    discardDownload(downloadId: string): Promise<{ removed: true }>;
    removeDownload(downloadId: string): Promise<{ removed: true }>;
    listDownloads(): Promise<DownloadRecord[]>;
    onDownloadProgress(fn: (record: DownloadRecord) => void): Unsubscribe;
  }
}
