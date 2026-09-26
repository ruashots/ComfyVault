import { afterEach, describe, expect, it } from "vitest";

import { fmt } from "~/domain/format";
import { ConfirmModalView } from "~/modals/confirm";
import { CleanupScreen, cleanupSummary } from "~/screens/Cleanup";
import { FixtureEngine } from "~/ipc/fixture/engine";
import { renderWithApp, waitFor, type Harness } from "~/test/render";

let harness: Harness | null = null;

afterEach(() => {
  harness?.unmount();
  harness = null;
});

/** Apply every group of the sample plan, then open Cleanup. */
async function afterARun(prepare?: (engine: FixtureEngine) => Promise<void> | void) {
  const engine = new FixtureEngine({ manual: true });
  engine.devSetSymlinksSupported(true);
  engine.devSetComfyRunning(false);
  const scan = (await engine.getLastScan())!;
  const plan = await engine.buildPlan(scan.scanId);
  await engine.startApply({
    planId: plan.planId,
    groupIds: plan.groups.map((g) => g.groupId),
  });
  engine.devFinish();
  await prepare?.(engine);
  harness = await renderWithApp(
    () => (
      <>
        <CleanupScreen />
        <ConfirmModalView />
      </>
    ),
    { engine },
  );
  await waitFor(() => harness!.app.vaultFiles().length > 0);
  return harness;
}

const titles = () => [...document.querySelectorAll(".sec .t")].map((t) => t.textContent);
const counts = () => [...document.querySelectorAll(".sec .n")].map((t) => t.textContent);

describe("the top line of Cleanup", () => {
  it("is made of sentences, not labels strung together", () => {
    expect(cleanupSummary(0, 1, 0)).toBe(
      "1 model has more than one name. Every vault file is linked.",
    );
    expect(cleanupSummary(2, 3, 1)).toBe(
      "2 links lead to nothing. 3 models have more than one name. 1 vault file is not linked from any install.",
    );
    expect(cleanupSummary(1, 0, 4)).toBe(
      "1 link leads to nothing. Every model has one name. 4 vault files are not linked from any install.",
    );
  });
});

describe("the Cleanup sections, after a run", () => {
  it("title each section with a phrase", async () => {
    const { app } = await afterARun();
    await waitFor(() => app.nameGroups().length > 0);
    expect(titles().slice(0, 2)).toEqual([
      "One model with more than one name",
      "Vault files that nothing links to",
    ]);
    expect(counts()[0]).toBe(
      `${app.nameGroups().length} ${app.nameGroups().length === 1 ? "model" : "models"}`,
    );
    const orphans = app.orphans();
    expect(counts()[1]).toBe(
      orphans.length === 0
        ? "none"
        : `${orphans.length} ${orphans.length === 1 ? "file" : "files"}, ${fmt(orphans.reduce((s, f) => s + f.sizeBytes, 0))}`,
    );
    for (const count of counts()) expect(count).not.toContain("·");
  });

  it("says what one model's names are, and where each is used, in a sentence", async () => {
    const { app } = await afterARun();
    await waitFor(() => app.nameGroups().length > 0);
    const group = app.nameGroups()[0]!;
    const card = document.querySelector(".cgrp")!;
    expect(card.querySelector(".ch")!.textContent).toContain(
      `${group.category}:the same file under ${group.names.length} names`,
    );
    const used = group.names.find((n) => n.usedByLinks > 0 && n.seenInInstalls.length > 0)!;
    const lines = [...card.querySelectorAll(".rs")].map((r) => r.textContent);
    expect(lines.some((l) => /^the name used in .+, where \d+ links? points? at it$/.test(l ?? ""))).toBe(
      true,
    );
    expect(used).toBeDefined();
    expect(card.textContent).not.toContain("·");
  });

  it("says a vault file nothing links to will be freed, not that it comes back", async () => {
    const { app } = await afterARun();
    await waitFor(() => app.orphans().length > 0);
    const orphan = app.orphans()[0]!;
    const row = [...document.querySelectorAll(".grp")].find(
      (g) => g.querySelector(".grp-n")?.textContent === orphan.canonicalName,
    )!;
    expect(row.querySelector(".grp-why")!.textContent).toMatch(
      new RegExp(`^in ${orphan.category}, in the vault since .+, and no install links to it$`),
    );
    (row.querySelector("button.dng") as HTMLButtonElement).click();
    await waitFor(() => document.querySelector(".modal") !== null);
    const modal = document.querySelector(".modal")!.textContent ?? "";
    expect(modal).toContain(`${orphan.canonicalName} will be deleted from the vault, and`);
    expect(modal).toContain(`${fmt(orphan.sizeBytes)} will be freed.`);
    expect(modal).not.toContain("comes back");
  });
});
