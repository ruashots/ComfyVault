import { describe, expect, it } from "vitest";

import { countOf } from "~/domain/format";

describe("a count with its word", () => {
  it("uses the one-form word for exactly one", () => {
    expect(countOf(1, "copy", "copies")).toBe("1 copy");
    expect(countOf(1, "model", "models")).toBe("1 model");
  });

  it("uses the many-form word for zero and for more than one", () => {
    expect(countOf(0, "file", "files")).toBe("0 files");
    expect(countOf(2, "link", "links")).toBe("2 links");
    expect(countOf(79, "copy", "copies")).toBe("79 copies");
  });
});
