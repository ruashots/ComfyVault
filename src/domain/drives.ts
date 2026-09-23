import type { DriveInfo } from "~/ipc/contract";

/**
 * What a drive is, in words, when it is not the ordinary kind.
 *
 * Only a fixed drive is always there. Everything else can be absent on any
 * given day, and the vault has to be present whenever ComfyUI runs, so the
 * interface names the kind wherever the choice is being made.
 */
export function driveKindWord(kind: DriveInfo["kind"]): string | null {
  switch (kind) {
    case "fixed":
      return null;
    case "removable":
      return "removable drive";
    case "network":
      return "network drive";
    case "optical":
      return "optical drive";
    case "ramDisk":
      return "RAM disk, which is emptied when the computer restarts";
    case "unknown":
      return "drive of a kind ComfyVault could not work out";
  }
}

/** The short form, for a line under a drive in the rail. */
export function driveKindShort(kind: DriveInfo["kind"]): string | null {
  switch (kind) {
    case "fixed":
      return null;
    case "ramDisk":
      return "RAM disk";
    case "unknown":
      return "kind not known";
    default:
      return `${kind} drive`;
  }
}

/** A drive that answered when asked how much room it has. */
export function isReadable(drive: DriveInfo): boolean {
  return drive.freeBytes !== null && drive.totalBytes !== null;
}

/** The drive a path is on, by its root, or null when nothing matches. */
export function driveFor(
  path: string,
  drives: readonly DriveInfo[],
): DriveInfo | null {
  const at = path.slice(0, 2).toUpperCase();
  return drives.find((d) => d.root.slice(0, 2).toUpperCase() === at) ?? null;
}
