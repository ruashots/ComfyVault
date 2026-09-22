/**
 * What to say about a copy that cannot move.
 *
 * The engine reports a reason as a machine-readable kind and the facts behind
 * it. Every sentence about it is written here, so no screen has to phrase one
 * of its own and no raw token ever reaches the glass.
 */

import type { BlockedReason } from "~/ipc/contract";

/** One word for the role column, next to keep and link. */
export function blockedRole(reason: BlockedReason): string {
  switch (reason.kind) {
    case "file_open":
      return "open";
    case "other_drive":
      return "drive";
    case "permission_denied":
      return "denied";
  }
}

/** The short reason, shown at the end of a plan row. */
export function blockedShort(reason: BlockedReason): string {
  switch (reason.kind) {
    case "file_open":
      return "the file is open";
    case "other_drive":
      return "on another drive";
    case "permission_denied":
      return "permission refused";
  }
}

/** The full reason, shown under a file that stays put. */
export function blockedWhy(
  reason: BlockedReason,
  instanceName: string,
): string {
  switch (reason.kind) {
    case "file_open":
      return `ComfyUI-${instanceName} is running and has this file open. Windows will not move a file that a program is using.`;
    case "other_drive":
      return `This copy sits on drive ${reason.drive}. The vault is on drive ${reason.vaultDrive}. Moving across drives is a copy, so the space on ${reason.vaultDrive} is used before the space on ${reason.drive} is freed.`;
    case "permission_denied":
      return "Windows refused to move this file. ComfyVault does not have write permission on the folder it is in.";
  }
}
