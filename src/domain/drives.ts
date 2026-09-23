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

/**
 * A drive named the way a person writes it: "C:".
 *
 * The engine's `volume` is whatever the operating system calls the volume a
 * path sits on. On Windows that is the drive root, "C:\" with a trailing
 * separator, so it can never be compared against a path's own first two
 * characters and must be trimmed before it is printed beside them.
 */
export function volumeLabel(volume: string | null | undefined): string {
  if (!volume) return "";
  return volume.replace(/[\\/]+$/, "");
}

/**
 * Whether two places are on one drive.
 *
 * Both sides go through the same normalising, because comparing a volume the
 * engine reported against a path the picker holds is comparing two differently
 * shaped strings: "C:\" is never equal to "C:", and an install on the vault's
 * own drive was told its files would be copied.
 */
export function sameVolume(
  a: string | null | undefined,
  b: string | null | undefined,
): boolean {
  if (!a || !b) return false;
  const of = (v: string) => v.slice(0, 2).toUpperCase();
  return of(a) === of(b) && /^[A-Za-z]:$/.test(of(a));
}
