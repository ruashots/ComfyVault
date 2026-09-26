/**
 * What the Download screen says, worked out from what the engine reports.
 *
 * The engine owns every decision: whether an address can be read, which file
 * it names, whether the bytes matched. This file only turns those answers into
 * sentences, and it is pure, so every sentence is testable on its own.
 *
 * The words follow the person's rules. What the person pastes is an address. A
 * link is only what the vault makes in an install. Before Download is pressed
 * the card is a plan, while it runs it says what is happening, and after it
 * says what happened.
 */

import { fmt, timeLeft } from "~/domain/format";
import { installNameOf } from "~/domain/installname";
import type { Install } from "~/ipc/contract";
import type { AddressPlan, DownloadRecord } from "~/ipc/draft";

/** Room left on the vault drive after the file, so Windows is never filled. */
export const SPACE_MARGIN_BYTES = 5 * 1024 ** 3;

export type Host = DownloadRecord["host"];

export function hostName(host: Host): string {
  return host === "huggingface" ? "Hugging Face" : "Civitai";
}

/**
 * Which service a pasted text points at, for the one sentence shown while the
 * engine reads it. The engine still decides whether the address can be used.
 */
export function hostOf(address: string): Host | null {
  const text = address.trim().toLowerCase();
  if (/^(https?:\/\/)?([a-z0-9-]+\.)*huggingface\.co(\/|$)/.test(text)) return "huggingface";
  if (/^(https?:\/\/)?([a-z0-9-]+\.)*civitai\.com(\/|$)/.test(text)) return "civitai";
  return null;
}

/** "ComfyUI-Easy-Install and ComfyUI-Flux", by the name each install is shown under. */
export function installList(ids: readonly string[], installs: readonly Install[]): string {
  return [...new Set(ids)].map((id) => installNameOf(id, installs)).join(" and ");
}

/** One stretch of a sentence, and the color it is said in. */
export interface Part {
  text: string;
  tone?: "now" | "ok" | "bad";
}

export type RowAction = "stop" | "remove" | "continue" | "discard" | "again" | "library";

export interface RowView {
  parts: Part[];
  /** Null when there is no progress to show. */
  bar: { fraction: number; stopped: boolean } | null;
  actions: RowAction[];
}

const MB = 1024 * 1024;

/** "38 MB/s", or null before the speed is known. */
export function speedOf(bytesPerSecond: number | null): string | null {
  if (bytesPerSecond === null || !Number.isFinite(bytesPerSecond) || bytesPerSecond <= 0) {
    return null;
  }
  const mb = bytesPerSecond / MB;
  return `${mb >= 10 ? Math.round(mb) : mb.toFixed(1)} MB/s`;
}

const fraction = (r: DownloadRecord) =>
  r.sizeBytes > 0 ? Math.min(1, Math.max(0, r.bytesDone / r.sizeBytes)) : 0;

/** What one row of the Downloads list says, and which buttons it offers. */
export function rowView(
  r: DownloadRecord,
  installs: readonly Install[],
  vaultVolume: string,
): RowView {
  const host = hostName(r.host);
  const at = `${fmt(r.bytesDone)} of ${fmt(r.sizeBytes)}`;
  const asked = installList(r.installIds, installs);
  const linked = installList(r.linkedInstallIds, installs);
  const kept = " The part already downloaded is kept.";
  const grey = { fraction: fraction(r), stopped: true };

  switch (r.state) {
    case "running": {
      const speed = speedOf(r.bytesPerSecond);
      const seconds =
        speed && r.bytesPerSecond ? (r.sizeBytes - r.bytesDone) / r.bytesPerSecond : null;
      return {
        parts: [
          { text: "Downloading:", tone: "now" },
          { text: ` ${at}, ${speed ? `${speed}, ` : ""}${timeLeft(seconds)}.` },
        ],
        bar: { fraction: fraction(r), stopped: false },
        actions: ["stop"],
      };
    }
    case "waiting":
      return {
        parts: [
          {
            text: `Will start when the download above is done. ${
              asked ? `Then it will be linked in ${asked}.` : "Then it will go into the vault."
            }`,
          },
        ],
        bar: null,
        actions: ["remove"],
      };
    case "checking":
      return {
        parts: [
          { text: "Checking", tone: "now" },
          {
            text: ` the SHA-256 of the downloaded file. ${
              asked ? "Then it goes into the vault and gets its links." : "Then it goes into the vault."
            }`,
          },
        ],
        bar: { fraction: 1, stopped: false },
        actions: [],
      };
    case "stopped":
      return {
        parts: [
          {
            text: `Stopped at ${at}. The part already downloaded is kept, so it can continue from there.`,
          },
        ],
        bar: grey,
        actions: ["continue", "discard"],
      };
    case "failed": {
      const error = r.error;
      const actions: RowAction[] = ["continue", "discard"];
      if (!error || error.kind === "connection") {
        return {
          parts: [
            { text: `The connection to ${host} dropped at ${at}.`, tone: "bad" },
            { text: kept },
          ],
          bar: grey,
          actions,
        };
      }
      if (error.kind === "noSpace") {
        return {
          parts: [
            { text: `Drive ${vaultVolume} ran out of space at ${at}.`, tone: "bad" },
            { text: `${kept} Free some space, then continue.` },
          ],
          bar: grey,
          actions,
        };
      }
      if ((error.kind === "refused" || error.kind === "expired") && error.serviceMessage) {
        return {
          parts: [
            { text: `${host} stopped the download at ${at}.`, tone: "bad" },
            { text: ` It says: "${error.serviceMessage}"${kept}` },
          ],
          bar: grey,
          actions,
        };
      }
      // Anything else is said in the engine's own sentence.
      return {
        parts: [
          { text: error.message, tone: "bad" },
          { text: error.kind === "changedOnSite" ? "" : kept },
        ],
        bar: grey,
        actions,
      };
    }
    case "mismatch":
      return {
        parts: [
          {
            text: `The downloaded file did not match the SHA-256 ${host} gave, so it was deleted.`,
            tone: "bad",
          },
          {
            text: ` Nothing went into the vault and nothing was linked. This happens when the file changed on ${host} or the transfer was damaged.`,
          },
        ],
        bar: null,
        actions: ["again", "remove"],
      };
    case "cutOff":
      return {
        parts: [{ text: `Cut off at ${at} when ComfyVault closed.${kept}` }],
        bar: grey,
        actions: ["continue", "discard"],
      };
    case "done":
      if (r.alreadyInVault === "after") return linkedOnly(linked, "so the new file was deleted");
      return {
        parts: [
          { text: "Downloaded", tone: "ok" },
          {
            text: ` into the vault as ${r.vaultRelPath}${linked ? `, and linked in ${linked}` : ""}.`,
          },
        ],
        bar: null,
        actions: ["library"],
      };
    case "linkedOnly":
      return linkedOnly(
        linked,
        r.alreadyInVault === "after"
          ? "so the new file was deleted"
          : "so nothing was downloaded",
      );
  }
}

function linkedOnly(linked: string, why: string): RowView {
  return {
    parts: linked
      ? [
          { text: "Linked", tone: "ok" },
          { text: ` in ${linked}. It was already in the vault, ${why}.` },
        ]
      : [{ text: `It was already in the vault, ${why}.`, tone: "ok" }],
    bar: null,
    actions: ["library"],
  };
}

const FINISHED: ReadonlySet<DownloadRecord["state"]> = new Set(["done", "linkedOnly"]);

/**
 * The order the list shows: what still needs the person or the queue first,
 * in the order it was started, then what is finished, newest first.
 */
export function listOrder(records: readonly DownloadRecord[]): DownloadRecord[] {
  const open = records.filter((r) => !FINISHED.has(r.state));
  const done = records.filter((r) => FINISHED.has(r.state)).reverse();
  return [...open, ...done];
}

/** The count beside the Downloads title. */
export function listCount(records: readonly DownloadRecord[]): string {
  const open = records.filter((r) => !FINISHED.has(r.state)).length;
  if (open === 0) return "all finished";
  return open === 1 ? "1 is not finished" : `${open} are not finished`;
}

/** Downloads under way or waiting their turn, for the badge on the rail. */
export function activeCount(records: readonly DownloadRecord[]): number {
  return records.filter(
    (r) => r.state === "running" || r.state === "waiting" || r.state === "checking",
  ).length;
}

/** The banner and the Home line for downloads ComfyVault closed on. */
export function cutOffSentence(records: readonly DownloadRecord[]): string | null {
  const cut = records.filter((r) => r.state === "cutOff");
  if (cut.length === 0) return null;
  if (cut.length === 1) {
    const r = cut[0]!;
    return `ComfyVault closed while ${r.fileName} was downloading. ${fmt(r.bytesDone)} of ${fmt(r.sizeBytes)} is kept. Continue it from there, or discard it.`;
  }
  return `ComfyVault closed while ${cut.length} downloads were not finished. The parts already downloaded are kept. Continue them from there, or discard them.`;
}

/** Home's one line for the same fact, and the words of its link. */
export function cutOffLine(
  records: readonly DownloadRecord[],
): { text: string; link: string } | null {
  const cut = records.filter((r) => r.state === "cutOff");
  if (cut.length === 0) return null;
  if (cut.length === 1) {
    const r = cut[0]!;
    return {
      text: `${r.fileName} was cut off at ${fmt(r.bytesDone)} of ${fmt(r.sizeBytes)} when ComfyVault closed.`,
      link: "Continue it or discard it",
    };
  }
  return {
    text: `${cut.length} downloads were cut off when ComfyVault closed.`,
    link: "Continue them or discard them",
  };
}

// ── the plan card ───────────────────────────────────────────────────────────

/** Whether the vault drive has room for the file and the margin. */
export function hasRoom(plan: Pick<AddressPlan, "sizeBytes" | "vaultFreeBytes">): boolean {
  return plan.vaultFreeBytes === null || plan.vaultFreeBytes - plan.sizeBytes >= SPACE_MARGIN_BYTES;
}

/** "Then it will be linked in 2 installs." */
export function linkHint(ticked: number): string | null {
  if (ticked === 0) return null;
  return `Then it will be linked in ${ticked === 1 ? "1 install" : `${ticked} installs`}.`;
}
