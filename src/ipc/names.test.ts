import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";

import { describe, expect, it } from "vitest";

/**
 * The interface and the engine must spell every field the same way.
 *
 * They cannot, on their own. Each side writes the names by hand, so the
 * fixture can only ever agree with the interface and a disagreement with the
 * engine shows up as an empty panel on someone's machine rather than as a red
 * test. `huggingface_cache_dirs` against `huggingFaceCacheDirs` did exactly
 * that: serde reads "huggingface" as one word, the field arrived undefined,
 * and three panels of Settings drew nothing.
 *
 * So this reads the engine's own Rust source, works out the names serde will
 * put on the wire, and compares them with the names written in the contract
 * file. It needs nothing committed by either side and it fails the day one of
 * them renames a field alone.
 */

const ROOT = join(import.meta.dirname, "..", "..");
const RUST_DIRS = [
  join(ROOT, "crates", "comfyvault-core", "src"),
  join(ROOT, "src-tauri", "src"),
];
const CONTRACT = join(ROOT, "src", "ipc", "contract.ts");

/**
 * Engine fields no screen reads, and interface fields no engine sends, each
 * one a decision rather than an accident. Anything not listed here is a
 * disagreement, and a disagreement is how a panel ends up empty.
 */
const AGREED: Record<string, readonly string[]> = {
  // The engine's field is `huggingface_cache_dirs` and serde reads
  // "huggingface" as one word. comfyvault-core is renaming it to the two-word
  // spelling. `cacheDirsOf` reads whichever arrives, so both names are correct
  // while the rename is in flight. Remove both entries once it has landed.
  Settings: ["huggingfaceCacheDirs", "huggingFaceCacheDirs"],

  // Every name the vault holds for this content. Cleanup reads them from
  // `list_name_groups`, which is the screen that acts on them.
  PlanGroup: ["vaultAliases", "distinctFiles"],
  // Size and modification time are what the engine re-checks at apply time.
  // No screen shows them, and the report shows the group's size instead.
  PlanLink: ["sharesBytesWithAnother", "sizeBytes", "mtimeNanos"],
  PlanSource: ["sizeBytes", "mtimeNanos"],
};

function rustFiles(dir: string): string[] {
  const out: string[] = [];
  for (const entry of readdirSync(dir)) {
    const path = join(dir, entry);
    if (statSync(path).isDirectory()) out.push(...rustFiles(path));
    else if (entry.endsWith(".rs") && entry !== "tests.rs") out.push(path);
  }
  return out;
}

const camel = (snake: string) =>
  snake.replace(/_([a-z0-9])/g, (_, c: string) => c.toUpperCase());

/** Every camelCase struct in the engine, as the field names serde will emit. */
function engineStructs(): Map<string, string[]> {
  const found = new Map<string, string[]>();
  for (const dir of RUST_DIRS) {
    for (const file of rustFiles(dir)) {
      const lines = readFileSync(file, "utf8").split("\n");
      for (let i = 0; i < lines.length; i += 1) {
        if (!/^#\[serde\(rename_all = "camelCase"\)\]/.test(lines[i]!.trim())) continue;
        const header = lines[i + 1]?.trim() ?? "";
        const name = /^pub struct (\w+)\s*\{/.exec(header)?.[1];
        if (!name) continue;

        const fields: string[] = [];
        let renamed: string | null = null;
        let skip = false;
        for (let j = i + 2; j < lines.length; j += 1) {
          const line = lines[j]!.trim();
          if (line === "}") break;
          const attr = /^#\[serde\(([^)]*)\)\]/.exec(line);
          if (attr) {
            renamed = /rename = "([^"]+)"/.exec(attr[1]!)?.[1] ?? renamed;
            if (/\bskip\b|skip_serializing(?!_if)/.test(attr[1]!)) skip = true;
            continue;
          }
          const field = /^pub (\w+):/.exec(line)?.[1];
          if (!field) continue;
          if (!skip) fields.push(renamed ?? camel(field));
          renamed = null;
          skip = false;
        }
        if (fields.length > 0) found.set(name, fields);
      }
    }
  }
  return found;
}

/** Every top-level key of every interface written in the contract file. */
function contractInterfaces(): Map<string, string[]> {
  const found = new Map<string, string[]>();
  const lines = readFileSync(CONTRACT, "utf8").split("\n");
  for (let i = 0; i < lines.length; i += 1) {
    const name = /^export interface (\w+)\s*\{/.exec(lines[i]!)?.[1];
    if (!name) continue;
    const keys: string[] = [];
    let depth = 1;
    for (let j = i + 1; j < lines.length && depth > 0; j += 1) {
      const line = lines[j]!;
      const trimmed = line.trim();
      if (depth === 1) {
        const key = /^(\w+)\??:/.exec(trimmed)?.[1];
        if (key) keys.push(key);
      }
      depth += (line.match(/\{/g)?.length ?? 0) - (line.match(/\}/g)?.length ?? 0);
    }
    found.set(name, keys);
  }
  return found;
}

const engine = engineStructs();
const contract = contractInterfaces();
const shared = [...engine.keys()].filter((name) => contract.has(name)).sort();

describe("the engine and the interface spell every field the same way", () => {
  it("does not carry a stale exception for a field that is gone", () => {
    for (const [name, fields] of Object.entries(AGREED)) {
      const known = new Set([
        ...(engine.get(name) ?? []),
        ...(contract.get(name) ?? []),
      ]);
      for (const field of fields) {
        expect(known.has(field), `${name}.${field} is on neither side any more`).toBe(
          true,
        );
      }
    }
  });

  it("finds payloads on both sides to compare", () => {
    expect(engine.size).toBeGreaterThan(10);
    expect(contract.size).toBeGreaterThan(10);
    // If this ever drops to nothing, the parser broke and every check below
    // became a test that cannot fail.
    expect(shared.length).toBeGreaterThan(8);
  });

  it.each(shared)("%s", (name) => {
    const theirs = new Set(engine.get(name)!);
    const mine = new Set(contract.get(name)!);
    const allowed = new Set(AGREED[name] ?? []);

    const missing = [...theirs].filter((f) => !mine.has(f) && !allowed.has(f));
    const invented = [...mine].filter((f) => !theirs.has(f) && !allowed.has(f));

    expect(
      { missing, invented },
      `${name}: the engine sends [${[...theirs].join(", ")}]`,
    ).toEqual({ missing: [], invented: [] });
  });
});
