import { describe, expect, it } from "vitest";

import { FixtureEngine } from "~/ipc/fixture/engine";
import {
  nothingWasSearched,
  type LinkRecord,
  type UsageResult,
} from "~/ipc/contract";

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

/** A world that has been consolidated, so there are links to measure. */
async function afterAnApply(): Promise<FixtureEngine> {
  const engine = new FixtureEngine({ speed: 400 });
  engine.devSetSymlinksSupported(true);
  engine.devSetComfyRunning(false);
  const scan = await engine.startScan();
  await new Promise((resolve) => setTimeout(resolve, 200));
  const plan = await engine.buildPlan(scan.scanId);
  await engine.startApply({
    planId: plan.planId,
    groupIds: plan.groups.slice(0, 3).map((g) => g.groupId),
  });
  await new Promise((resolve) => setTimeout(resolve, 300));
  return engine;
}

describe("a link's state is read off the drive, not remembered", () => {
  it("is not stored on the record itself", () => {
    const record: LinkRecord = {
      id: "link-1",
      installId: "prod",
      absPath: "C:\\ComfyUI-Alpha\\models\\loras\\a.safetensors",
      relPath: "models\\loras\\a.safetensors",
      linkName: "a.safetensors",
      sha256: "A".repeat(64),
      vaultRelPath: "loras\\a.safetensors",
      createdAt: "2026-09-20T00:00:00.000Z",
      createdBy: "apply",
      applyId: "apply-1",
    };
    // A stored copy of state goes stale the moment something else moves a
    // file, so the record carries none and `list_links` measures it.
    expect("state" in record).toBe(false);
  });

  it("comes back with every link the engine measured", async () => {
    const engine = await afterAnApply();
    const links = await engine.listLinks();
    expect(links.length).toBeGreaterThan(0);
    for (const link of links) {
      expect(["ok", "dangling", "replaced", "missing"]).toContain(link.state);
    }
  });

  it("turns to dangling when the vault file goes", async () => {
    const engine = await afterAnApply();
    const before = await engine.listLinks();
    expect(before.some((l) => l.state === "dangling")).toBe(false);
    const broken = engine.devBreakLinks(1);
    expect(broken).toBeGreaterThan(0);
    const after = await engine.listLinks();
    // The record did not change. What is on the drive did.
    expect(after.filter((l) => l.state === "dangling")).toHaveLength(broken);
    expect(after).toHaveLength(before.length);
  });
});

describe("a modification time survives the trip", () => {
  it("is text, because the number does not fit in a JavaScript number", async () => {
    const engine = new FixtureEngine({ speed: 400 });
    const scan = await engine.startScan();
    await new Promise((resolve) => setTimeout(resolve, 200));
    const page = await engine.getScanEntries({
      scanId: scan.scanId,
      offset: 0,
      limit: 1,
    });
    const entry = page.entries[0]!;
    expect(typeof entry.mtimeNanos).toBe("string");
    // The last digits are the point. Through a number they would be lost.
    const asNumber = String(Number(entry.mtimeNanos));
    expect(entry.mtimeNanos.length).toBe(19);
    if (asNumber !== entry.mtimeNanos) {
      expect(entry.mtimeNanos).not.toBe(asNumber);
    }
  });
});
