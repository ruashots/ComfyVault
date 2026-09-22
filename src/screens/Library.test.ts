import { describe, expect, it } from "vitest";

import { buildLibrary, categoriesOf, type ContentRow } from "~/domain/view";
import { FixtureEngine } from "~/ipc/fixture/engine";
import { libraryRows, type LibraryFilters } from "~/screens/Library";
import type { UsageResult } from "~/ipc/contract";

async function library(): Promise<{
  rows: ContentRow[];
  usage: Map<string, UsageResult>;
}> {
  const engine = new FixtureEngine();
  engine.devSetSymlinksSupported(true);
  engine.devSetComfyRunning(false);
  const scan = (await engine.getLastScan())!;
  const plan = await engine.buildPlan(scan.scanId);
  const files = (await engine.listVaultFiles({ offset: 0, limit: 1000 })).files;
  const rows = buildLibrary(plan, files);
  const answers = await engine.checkModelUsage([
    ...new Set(rows.flatMap((r) => r.allNames)),
  ]);
  return { rows, usage: new Map(answers.map((a) => [a.name, a])) };
}

const view = (over: Partial<LibraryFilters> = {}): LibraryFilters => ({
  query: "",
  category: "all",
  unusedOnly: false,
  sort: "size",
  ...over,
});

describe("the library list with no filter", () => {
  it("shows every unique content once", async () => {
    const { rows, usage } = await library();
    const listed = libraryRows(rows, view(), usage);
    expect(listed).toHaveLength(rows.length);
    expect(new Set(listed.map((r) => r.sha256)).size).toBe(listed.length);
  });

  it("puts the biggest first", async () => {
    const { rows, usage } = await library();
    const listed = libraryRows(rows, view(), usage);
    for (let i = 1; i < listed.length; i++) {
      expect(listed[i - 1]!.bytes).toBeGreaterThanOrEqual(listed[i]!.bytes);
    }
  });
});

describe("sorting", () => {
  it("sorts by name", async () => {
    const { rows, usage } = await library();
    const names = libraryRows(rows, view({ sort: "name" }), usage).map((r) => r.name);
    expect(names).toEqual([...names].sort((a, b) => a.localeCompare(b)));
  });

  it("sorts by how many places hold it, biggest file breaking a tie", async () => {
    const { rows, usage } = await library();
    const listed = libraryRows(rows, view({ sort: "links" }), usage);
    for (let i = 1; i < listed.length; i++) {
      const before = listed[i - 1]!;
      const after = listed[i]!;
      expect(before.places.length).toBeGreaterThanOrEqual(after.places.length);
      if (before.places.length === after.places.length) {
        expect(before.bytes).toBeGreaterThanOrEqual(after.bytes);
      }
    }
  });
});

describe("searching", () => {
  it("finds a model by part of its name", async () => {
    const { rows, usage } = await library();
    const listed = libraryRows(rows, view({ query: "flux1-kontext" }), usage);
    expect(listed.map((r) => r.name)).toEqual(["flux1-kontext-dev.safetensors"]);
  });

  it("ignores capitals and the space around the words", async () => {
    const { rows, usage } = await library();
    expect(libraryRows(rows, view({ query: "  FLUX1-KONTEXT " }), usage)).toHaveLength(1);
  });

  it("finds a file by a name only one install uses", async () => {
    const { rows, usage } = await library();
    const listed = libraryRows(rows, view({ query: "Wan2_1_VAE_bf16" }), usage);
    expect(listed).toHaveLength(1);
    expect(listed[0]!.allNames).toContain("Wan2_1_VAE_bf16.safetensors");
  });

  it("returns nothing when nothing matches, rather than everything", async () => {
    const { rows, usage } = await library();
    expect(libraryRows(rows, view({ query: "no such model" }), usage)).toHaveLength(0);
  });
});

describe("filtering", () => {
  it("keeps only one folder", async () => {
    const { rows, usage } = await library();
    const listed = libraryRows(rows, view({ category: "vae" }), usage);
    expect(listed.length).toBeGreaterThan(0);
    for (const row of listed) expect(row.category).toBe("vae");
  });

  it("keeps only the models no workflow names", async () => {
    const { rows, usage } = await library();
    const listed = libraryRows(rows, view({ unusedOnly: true }), usage);
    expect(listed.length).toBeGreaterThan(0);
    for (const row of listed) expect(usage.get(row.name)?.used).toBe(false);
  });

  it("applies the search, the folder and the unused filter together", async () => {
    const { rows, usage } = await library();
    const listed = libraryRows(
      rows,
      view({ query: "sd3.5", category: "checkpoints", unusedOnly: true }),
      usage,
    );
    expect(listed.map((r) => r.name).sort()).toEqual([
      "sd3.5_large_turbo.safetensors",
      "sd3.5_medium.safetensors",
    ]);
  });

  it("can filter down to nothing", async () => {
    const { rows, usage } = await library();
    expect(
      libraryRows(rows, view({ query: "flux1-kontext", category: "vae" }), usage),
    ).toHaveLength(0);
  });

  it("never changes the rows it was given", async () => {
    const { rows, usage } = await library();
    const before = rows.map((r) => r.sha256);
    libraryRows(rows, view({ sort: "name" }), usage);
    libraryRows(rows, view({ sort: "links" }), usage);
    expect(rows.map((r) => r.sha256)).toEqual(before);
  });

  it("offers every category that is actually present, sorted", async () => {
    const { rows } = await library();
    const categories = categoriesOf(rows);
    expect(categories).toEqual([...categories].sort());
    expect(categories).toContain("vae");
    expect(categories).not.toContain("");
  });
});
