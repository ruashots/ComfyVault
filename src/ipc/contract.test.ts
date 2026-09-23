import { describe, expect, it } from "vitest";

import { nothingWasSearched, type UsageResult } from "~/ipc/contract";

const answer = (over: Partial<UsageResult>): UsageResult => ({
  name: "flux1-dev.safetensors",
  used: false,
  searched: true,
  matches: [],
  method: "The file name was searched for as plain text inside saved workflow files.",
  ...over,
});

describe("whether anything was searched at all", () => {
  it("reads the flag, not the sentence", () => {
    // A sentence the interface has never seen. The flag still decides.
    expect(
      nothingWasSearched(answer({ searched: false, method: "Nothing to look in." })),
    ).toBe(true);
  });

  it("does not call a real answer 'not checked' because of how it is worded", () => {
    // The engine searched, and said so with the words the old code matched on.
    expect(
      nothingWasSearched(
        answer({
          searched: true,
          method:
            "No saved workflow files were found for one install, so that one was skipped.",
        }),
      ),
    ).toBe(false);
  });

  it("is false for a model that was searched for and found", () => {
    expect(
      nothingWasSearched(
        answer({
          used: true,
          matches: [
            {
              installId: "prod",
              installLabel: "Production",
              workflowPath: "C:\\ComfyUI-Alpha\\user\\default\\workflows\\a.json",
              workflowName: "a.json",
            },
          ],
        }),
      ),
    ).toBe(false);
  });
});
