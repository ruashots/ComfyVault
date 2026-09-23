import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";

import { describe, expect, it } from "vitest";

/**
 * Every payload the engine sends, as the engine's own serialiser writes it.
 *
 * `docs/golden/` holds one file per payload, produced by the real serde code
 * rather than written by hand. The sibling test in `names.test.ts` reads the
 * engine's Rust and catches a rename the day it happens. This one catches what
 * a name cannot show: whether a field is a string or a number, whether it can
 * be null, and whether it is a list. `mtimeNanos` is the reason both exist. It
 * kept its name and changed from a number to text, because JavaScript loses
 * the last digits of 1758240123456789012.
 */

const GOLDEN = join(import.meta.dirname, "..", "..", "docs", "golden");
const CONTRACT = join(import.meta.dirname, "contract.ts");

/**
 * Simple aliases, so a named type cannot hide what it really is. `NanoTime` is
 * why: as an opaque name it would accept a number just as happily as text,
 * which is the exact mistake this test exists to catch.
 */
function aliases(): Map<string, string> {
  const found = new Map<string, string>();
  for (const line of readFileSync(CONTRACT, "utf8").split("\n")) {
    const match = /^export type (\w+) = (.+);$/.exec(line);
    if (match) found.set(match[1]!, match[2]!);
  }
  return found;
}

/** Each interface in the contract file, as key to the type written after it. */
function contractTypes(): Map<string, Map<string, string>> {
  const found = new Map<string, Map<string, string>>();
  const lines = readFileSync(CONTRACT, "utf8").split("\n");
  for (let i = 0; i < lines.length; i += 1) {
    const name = /^export interface (\w+)\s*\{/.exec(lines[i]!)?.[1];
    if (!name) continue;
    const fields = new Map<string, string>();
    let depth = 1;
    for (let j = i + 1; j < lines.length && depth > 0; j += 1) {
      const line = lines[j]!;
      if (depth === 1) {
        const match = /^\s*(\w+)(\??):\s*(.+?);?\s*$/.exec(line);
        if (match) fields.set(match[1]!, `${match[3]!}${match[2] ? " | undefined" : ""}`);
      }
      depth += (line.match(/\{/g)?.length ?? 0) - (line.match(/\}/g)?.length ?? 0);
    }
    found.set(name, fields);
  }
  return found;
}

/** What a JSON value is, in the words a TypeScript type would use. */
function kindOf(value: unknown): string {
  if (value === null) return "null";
  if (Array.isArray(value)) return "array";
  return typeof value;
}

/** A named type this test cannot resolve, so it judges only the name. */
const opaque = (type: string) =>
  /^[A-Z]/.test(type.replace(/\breadonly\s+/g, "").trim()) ||
  type.includes("{") ||
  type.includes('"');

function accepts(type: string, kind: string): boolean {
  const t = type.replace(/\breadonly\s+/g, "");
  switch (kind) {
    case "null":
      return /\bnull\b/.test(t);
    case "array":
      return /\[\]|Array</.test(t);
    case "string":
      return /\bstring\b/.test(t) || opaque(t);
    case "number":
      return /\bnumber\b/.test(t);
    case "boolean":
      return /\bboolean\b/.test(t);
    default:
      return true;
  }
}

const ALIASES = aliases();

/** A type with its simple aliases replaced by what they stand for. */
function resolve(type: string): string {
  let out = type;
  for (let pass = 0; pass < 3; pass += 1) {
    const next = out.replace(/\b[A-Z]\w*\b/g, (name) => ALIASES.get(name) ?? name);
    if (next === out) break;
    out = next;
  }
  return out;
}

const types = contractTypes();
const samples = readdirSync(GOLDEN)
  .filter((f) => f.endsWith(".json"))
  .map((f) => f.replace(/\.json$/, ""))
  .sort();
const shared = samples.filter((name) => types.has(name));

describe("every payload matches the sample the engine's serialiser wrote", () => {
  it("sees through a simple alias to what it really is", () => {
    expect(ALIASES.get("NanoTime")).toBe("string");
    expect(accepts(resolve("NanoTime"), "string")).toBe(true);
    expect(accepts(resolve("NanoTime"), "number")).toBe(false);
  });

  it("has samples to compare against", () => {
    expect(samples.length).toBeGreaterThan(40);
    // If this collapses, every check below became a test that cannot fail.
    expect(shared.length).toBeGreaterThan(20);
  });

  it.each(shared)("%s", (name) => {
    const sample = JSON.parse(
      readFileSync(join(GOLDEN, `${name}.json`), "utf8"),
    ) as Record<string, unknown>;
    const declared = types.get(name)!;

    const missing = Object.keys(sample).filter((key) => !declared.has(key));
    expect(missing, `${name}: the engine sends these and the contract has not`).toEqual(
      [],
    );

    const wrong: string[] = [];
    for (const [key, value] of Object.entries(sample)) {
      const type = resolve(declared.get(key)!);
      const kind = kindOf(value);
      if (!accepts(type, kind)) wrong.push(`${key} is ${kind}, declared "${type}"`);
    }
    expect(wrong, `${name}: the sample disagrees with the contract`).toEqual([]);
  });
});

describe("a hash is written the way the engine writes it", () => {
  it("is uppercase in every sample that carries one", () => {
    for (const name of samples) {
      const text = readFileSync(join(GOLDEN, `${name}.json`), "utf8");
      for (const [, hash] of text.matchAll(/"sha256":\s*"([0-9a-fA-F]{64})"/g)) {
        expect(hash, `${name} carries a lowercase hash`).toBe(hash!.toUpperCase());
      }
    }
  });
});
