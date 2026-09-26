/**
 * The name an install is shown under.
 *
 * The engine names an install after its root folder, and a launcher layout puts
 * the real root in a folder called ComfyUI, so two or three installs can all be
 * "ComfyUI". A screen that shows three rows called ComfyUI tells the person
 * nothing. This finds the folder that tells them apart.
 */

interface Nameable {
  id: string;
  label: string;
  root: string;
}

const same = (a: string, b: string) => a.toLowerCase() === b.toLowerCase();

/** "C:\AI\Easy\ComfyUI" -> ["C:", "AI", "Easy", "ComfyUI"] */
const foldersOf = (root: string) => root.split(/[\\/]+/).filter((s) => s.length > 0);

/**
 * The label, when no other install has it. Otherwise walk up from the root one
 * folder at a time, and take the first folder name no other install has at the
 * same level and no other install is called. Last, the whole root.
 */
export function installName(install: Nameable, all: readonly Nameable[]): string {
  const others = all.filter((o) => o.id !== install.id);
  if (!others.some((o) => same(o.label, install.label))) return install.label;

  const mine = foldersOf(install.root);
  const theirs = others.map((o) => foldersOf(o.root));
  // The first folder is the drive, which is not a name.
  for (let up = 1; up < mine.length; up++) {
    const name = mine[mine.length - up]!;
    const atSameLevel = theirs.some((t) => {
      const other = t[t.length - up];
      return other !== undefined && same(other, name);
    });
    if (atSameLevel) continue;
    if (others.some((o) => same(o.label, name))) continue;
    return name;
  }
  return install.root;
}

/** The name of the install with this id, or the id when it is not registered. */
export function installNameOf(id: string, all: readonly Nameable[]): string {
  const install = all.find((i) => i.id === id);
  return install ? installName(install, all) : id;
}
