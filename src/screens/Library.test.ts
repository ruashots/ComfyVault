import { describe, expect, it } from "vitest";

import { derivePlan } from "~/domain/plan";
import { fixtureMachine, fixtureScan } from "~/ipc/fixture/dataset";
import { libraryRows } from "~/screens/Library";
import { BLESSED } from "~/test/blessed";
import type { LibrarySort } from "~/state/store";

const plan = derivePlan(fixtureScan(), fixtureMachine());

const view = (over: Partial<{
  query: string;
  folder: string;
  unusedOnly: boolean;
  sort: LibrarySort;
}> = {}) => ({
  query: "",
  folder: "all",
  unusedOnly: false,
  sort: "size" as LibrarySort,
  ...over,
});

describe("the library list with no filter", () => {
  it("shows every model once", () => {
    expect(libraryRows(plan, view())).toHaveLength(BLESSED.models);
  });

  it("puts the biggest first", () => {
    const rows = libraryRows(plan, view());
    expect(rows[0]!.filename).toBe("wan2.1_i2v_480p_14B_bf16.safetensors");
    for (let i = 1; i < rows.length; i++) {
      expect(rows[i - 1]!.bytes).toBeGreaterThanOrEqual(rows[i]!.bytes);
    }
  });
});

describe("sorting", () => {
  it("sorts by name", () => {
    const rows = libraryRows(plan, view({ sort: "name" }));
    const names = rows.map((r) => r.filename);
    expect(names).toEqual([...names].sort((a, b) => a.localeCompare(b)));
  });

  it("sorts by how many places hold it, biggest file breaking a tie", () => {
    const rows = libraryRows(plan, view({ sort: "links" }));
    for (let i = 1; i < rows.length; i++) {
      const before = rows[i - 1]!;
      const after = rows[i]!;
      const a = before.model.placements.length;
      const b = after.model.placements.length;
      expect(a).toBeGreaterThanOrEqual(b);
      if (a === b) expect(before.bytes).toBeGreaterThanOrEqual(after.bytes);
    }
    expect(rows[0]!.model.placements.length).toBe(3);
  });
});

describe("searching", () => {
  it("finds a model by part of its name", () => {
    const rows = libraryRows(plan, view({ query: "flux1-kontext" }));
    expect(rows.map((r) => r.filename)).toEqual([
      "flux1-kontext-dev.safetensors",
    ]);
  });

  it("ignores capitals", () => {
    expect(libraryRows(plan, view({ query: "FLUX1-KONTEXT" }))).toHaveLength(1);
  });

  it("ignores space around the words", () => {
    expect(libraryRows(plan, view({ query: "  clip_l  " }))).toHaveLength(1);
  });

  it("finds a file by a name only one install uses", () => {
    const rows = libraryRows(plan, view({ query: "Wan2_1_VAE_bf16" }));
    expect(rows.map((r) => r.filename)).toEqual(["wan_2.1_vae.safetensors"]);
  });

  it("returns nothing when nothing matches, rather than everything", () => {
    expect(libraryRows(plan, view({ query: "no such model" }))).toHaveLength(0);
  });
});

describe("filtering", () => {
  it("keeps only one folder", () => {
    const rows = libraryRows(plan, view({ folder: "vae" }));
    expect(rows.length).toBeGreaterThan(0);
    for (const row of rows) expect(row.folder).toBe("vae");
  });

  it("keeps only the models no workflow names", () => {
    const rows = libraryRows(plan, view({ unusedOnly: true }));
    expect(rows).toHaveLength(BLESSED.unused);
    for (const row of rows) expect(row.model.workflowHits).toBe(0);
  });

  it("applies the search, the folder and the unused filter together", () => {
    const rows = libraryRows(
      plan,
      view({ query: "sd3.5", folder: "checkpoints", unusedOnly: true }),
    );
    expect(rows.map((r) => r.filename)).toEqual([
      "sd3.5_large_turbo.safetensors",
      "sd3.5_medium.safetensors",
    ]);
  });

  it("can filter down to nothing", () => {
    const rows = libraryRows(
      plan,
      view({ query: "flux1-kontext", folder: "vae" }),
    );
    expect(rows).toHaveLength(0);
  });

  it("never changes the plan it was given", () => {
    const before = plan.models.map((m) => m.id);
    libraryRows(plan, view({ sort: "name" }));
    libraryRows(plan, view({ sort: "links" }));
    expect(plan.models.map((m) => m.id)).toEqual(before);
  });
});
