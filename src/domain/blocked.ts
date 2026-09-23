/**
 * What to say about a file the engine will not move.
 *
 * The engine reports a machine-readable reason and a technical detail. Every
 * sentence a person reads about it is written here, so no screen phrases one of
 * its own and no token from the engine ever reaches the glass.
 */

import type { BlockReason, BlockedRow } from "~/ipc/contract";

/** One word for the role column, next to keep and link. */
export function blockedRole(reason: BlockReason): string {
  switch (reason) {
    case "fileLocked":
      return "open";
    case "fileChanged":
      return "changed";
    case "fileMissing":
      return "gone";
    case "permissionDenied":
      return "denied";
    case "inCustomNodes":
      return "node";
    case "inHuggingFaceCache":
      return "cache";
    case "alreadyInVault":
      return "linked";
    case "externalLink":
      return "link";
    case "symlinkUnsupported":
      return "links off";
    case "vaultInsideInstall":
      return "vault";
    case "targetExistsNotLink":
      return "in the way";
    case "notEnoughSpace":
      return "no room";
    case "readError":
      return "unread";
    case "unsafeVaultPath":
      return "escapes";
  }
}

/** The short reason, shown at the end of a plan row. */
export function blockedShort(reason: BlockReason): string {
  switch (reason) {
    case "fileLocked":
      return "the file is open";
    case "fileChanged":
      return "changed since the scan";
    case "fileMissing":
      return "no longer there";
    case "permissionDenied":
      return "permission refused";
    case "inCustomNodes":
      return "inside custom_nodes";
    case "inHuggingFaceCache":
      return "in the Hugging Face cache";
    case "alreadyInVault":
      return "already a link";
    case "externalLink":
      return "points somewhere else";
    case "symlinkUnsupported":
      return "links are off";
    case "vaultInsideInstall":
      return "the vault is inside this install";
    case "targetExistsNotLink":
      return "the vault name is taken";
    case "notEnoughSpace":
      return "not enough room";
    case "readError":
      return "could not be read";
    case "unsafeVaultPath":
      return "its folder name escapes the vault";
  }
}

/** The full reason, shown under a file that stays put. */
export function blockedWhy(row: BlockedRow): string {
  const who = row.installLabel ?? "that install";
  switch (row.reason) {
    case "fileLocked":
      return `ComfyUI is running out of ${who} and has this file open. Windows will not move a file that a program is using. Close it and check again.`;
    case "fileChanged":
      return "This file was written to after the scan read it, so what is on disk no longer matches what the report checked. Run the scan again to pick up the new version.";
    case "fileMissing":
      return "This file was there when the scan ran and it is not there now. Something else moved or deleted it. Run the scan again.";
    case "permissionDenied":
      return "Windows refused to touch this file. ComfyVault does not have write permission on the folder it is in.";
    case "inCustomNodes":
      return "This weight sits inside a custom node's own folder. A node can load it straight from there, so ComfyVault counts it and leaves it alone.";
    case "inHuggingFaceCache":
      return "This weight sits in the Hugging Face cache, which the libraries manage themselves. ComfyVault counts it and leaves it alone.";
    case "alreadyInVault":
      return "This path already holds a link into the vault. There is nothing left to do here.";
    case "externalLink":
      return "This path is a link that points somewhere outside the vault. ComfyVault will not replace a link somebody else made.";
    case "symlinkUnsupported":
      return "Windows will not let ComfyVault create a link right now, so nothing can move. The report is complete and stays true once links are turned on.";
    case "vaultInsideInstall":
      return "The vault folder sits inside this install. Moving a file into it would leave the file inside the same install it came from. Choose a vault folder of its own in Settings.";
    case "targetExistsNotLink":
      return "Something already sits at the name this file would take inside the vault, and it is not a link. ComfyVault never overwrites. Rename one of them and scan again.";
    case "notEnoughSpace":
      return "There is not enough free room on the vault's drive to take this file across. Free some space, or put the vault on the same drive as the install.";
    case "readError":
      return "ComfyVault could not read this file, so it cannot tell what it holds.";
    case "unsafeVaultPath":
      return `The folder this file would take inside the vault is named by a category in the extra_model_paths.yaml that ${who} uses, and that name points back out of the vault. ComfyVault never writes outside its own folder. Fix the category name in that file and scan again.`;
  }
}

/**
 * Whether this row belongs in the "Cannot move" list.
 *
 * Weights inside custom_nodes and in the Hugging Face cache are counted in
 * their own section instead, because there are thousands of them and none of
 * them is a problem. Links being unavailable is one cause with one fix, and it
 * already has a panel of its own at the top of the screen, so listing every
 * file it touches would be the same sentence a thousand times.
 */
export function isSkippedByDesign(reason: BlockReason): boolean {
  return (
    reason === "inCustomNodes" ||
    reason === "inHuggingFaceCache" ||
    reason === "symlinkUnsupported" ||
    // Already a link into the vault, which is the finished state, not a stuck
    // one. Every path reads this way after a successful run, so listing them
    // put the whole tree under "cannot move" on the screen that says it worked.
    reason === "alreadyInVault"
  );
}

/** Blocked rows are listed grouped by reason, in this order. */
const ORDER: readonly BlockReason[] = [
  "symlinkUnsupported",
  "vaultInsideInstall",
  "unsafeVaultPath",
  "notEnoughSpace",
  "fileLocked",
  "permissionDenied",
  "fileChanged",
  "fileMissing",
  "targetExistsNotLink",
  "externalLink",
  "readError",
  "alreadyInVault",
  "inCustomNodes",
  "inHuggingFaceCache",
];

export function blockedRank(reason: BlockReason): number {
  const at = ORDER.indexOf(reason);
  return at < 0 ? ORDER.length : at;
}

/** Can the person do something about this one right now? */
export function isFixable(reason: BlockReason): boolean {
  return (
    reason === "fileLocked" ||
    reason === "permissionDenied" ||
    reason === "fileChanged" ||
    reason === "fileMissing" ||
    reason === "symlinkUnsupported"
  );
}
