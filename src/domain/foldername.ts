/**
 * What makes a new folder name unusable, said in words the person can act on.
 *
 * The person never types a path. They type a name, and the picker puts it inside
 * the folder they selected. Everything Windows refuses is refused here first, with
 * the reason, so no name reaches the engine only to come back as an error.
 */

import { joinPath } from "~/domain/format";

/** Names Windows keeps for devices. None of them can be a folder. */
const RESERVED = new Set([
  "con", "prn", "aux", "nul",
  "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8", "com9",
  "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
]);

const ILLEGAL = /[\\/:*?"<>|]/;

/**
 * Returns the reason the name cannot be used, or null when it can.
 * `siblings` are the paths already inside the parent folder.
 */
export function folderNameError(
  raw: string,
  parent: string,
  siblings: readonly string[],
): string | null {
  const name = (raw ?? "").trim();
  if (!name) return "Type a name for the folder.";
  if (ILLEGAL.test(name)) {
    return 'A folder name cannot contain \\ / : * ? " < > |';
  }
  if (/^\s|\s$/.test(raw)) {
    return "A folder name cannot start or end with a space.";
  }
  if (name.endsWith(".")) return "A folder name cannot end with a dot.";
  if (name.length > 64) {
    return "That name is too long. Keep it under 64 characters.";
  }
  const stem = name.split(".")[0]!.toLowerCase();
  if (RESERVED.has(stem)) {
    return `Windows keeps the name ${name} for itself. Choose another one.`;
  }
  const target = joinPath(parent, name).toLowerCase();
  if (siblings.some((p) => p.toLowerCase() === target)) {
    return `There is already a folder called ${name} here.`;
  }
  return null;
}
