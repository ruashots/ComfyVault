import { describe, expect, it } from "vitest";

import {
  hiddenKeyOf,
  nameCardTitle,
  nameCardsOf,
  nameCardsSummary,
  takenLines,
  unifyResultLine,
  unifyViewOf,
  usedByLine,
} from "~/domain/names";
import type { NameGroup, UnifyPlan, UnifyStep, UsageResult } from "~/ipc/contract";

const CONV = "example_upscaler_v1_fp16.safetensors";
const FP16 = "example_upscaler_fp16.safetensors";
const SHA = "A".repeat(64);

const installs = [
  { id: "normal", label: "ComfyUI-Beta", root: "C:\\ComfyUI-Beta" },
  { id: "prod", label: "ComfyUI-Alpha", root: "C:\\ComfyUI-Alpha" },
];

/** The person's model: Beta and Alpha use one name, Beta the other too. */
function ownerGroup(): NameGroup {
  return {
    sha256: SHA,
    sizeBytes: 100,
    category: "latent_upscale_models",
    canonicalName: FP16,
    names: [
      { name: FP16, isCanonical: true, vaultRelPath: "", usedByLinks: 1, seenInInstalls: ["normal"] },
      { name: CONV, isCanonical: false, vaultRelPath: "", usedByLinks: 2, seenInInstalls: ["normal", "prod"] },
    ],
  };
}

describe("which models get a card", () => {
  it("gives a card to a model its installs use two names for, and picks the name most installs use", () => {
    const [card] = nameCardsOf([ownerGroup()], []);
    expect(card!.names.map((n) => n.name)).toEqual([FP16, CONV]);
    expect(card!.initial).toBe(CONV);
    expect(nameCardTitle(card!)).toBe("ONE MODEL, TWO NAMES");
    expect(usedByLine(card!.names[1]!, installs)).toBe("ComfyUI-Beta · ComfyUI-Alpha");
  });

  it("leaves out a name no install uses, and a model left with one name", () => {
    const group = ownerGroup();
    group.names[0]!.seenInInstalls = [];
    expect(nameCardsOf([group], [])).toEqual([]);
  });

  it("hides a card kept as it is, until the installs use another name", () => {
    const [card] = nameCardsOf([ownerGroup()], []);
    const key = hiddenKeyOf(card!);
    expect(key).toEqual({ sha256: SHA, names: [CONV, FP16] });
    expect(nameCardsOf([ownerGroup()], [key])).toEqual([]);

    const grown = ownerGroup();
    grown.names.push({ name: "third.safetensors", isCanonical: false, vaultRelPath: "", usedByLinks: 1, seenInInstalls: ["prod"] });
    const [back] = nameCardsOf([grown], [key]);
    expect(back!.names).toHaveLength(3);
    expect(nameCardTitle(back!)).toBe("ONE MODEL, THREE NAMES");
  });

  it("says in the top line how many models have more than one name", () => {
    expect(nameCardsSummary([])).toBeNull();
    expect(nameCardsSummary(nameCardsOf([ownerGroup()], []))).toBe(
      "1 model has two names in your installs.",
    );
    const other = { ...ownerGroup(), sha256: "B".repeat(64) };
    expect(nameCardsSummary(nameCardsOf([ownerGroup(), other], []))).toBe(
      "2 models have more than one name in your installs.",
    );
  });
});

const step = (installId: string, linkName: string, action: UnifyStep["action"]): UnifyStep => ({
  installId,
  absPath: `C:\\${installId}\\models\\${linkName}`,
  linkName,
  action,
  newAbsPath: action === "rename" ? `C:\\${installId}\\models\\${CONV}` : null,
  takenBy: action === "blockedTaken" ? `C:\\${installId}\\models\\${CONV}` : null,
});

const usage = (name: string, matches: [string, string][], searched = true): UsageResult => ({
  name,
  used: matches.length > 0,
  searched,
  matches: matches.map(([installId, workflowName]) => ({
    installId,
    installLabel: installId,
    workflowPath: `C:\\${installId}\\user\\${workflowName}`,
    workflowName,
  })),
  method: searched ? "Searched the saved workflows." : "There were no saved workflow files to search.",
});

function plan(steps: UnifyStep[], workflows: UsageResult[], running: string[] = []): UnifyPlan {
  return { sha256: SHA, name: CONV, steps, running, workflows };
}

describe("what the dialog says", () => {
  it("asks to use the name in both installs, and lists the workflows to fix", () => {
    const view = unifyViewOf(
      plan(
        [step("normal", CONV, "keep"), step("prod", CONV, "keep"), step("normal", FP16, "rename")],
        [usage(FP16, [["normal", "upscale_3d_scene.json"], ["normal", "detail_pass_3d.json"]])],
      ),
      installs,
    );
    expect(view).toEqual({
      kind: "confirm",
      heading: [{ text: "Use this name in both installs?" }],
      name: CONV,
      taken: null,
      goingAway: [FP16],
      inInstalls: null,
      workflows: ["upscale_3d_scene.json", "detail_pass_3d.json"],
      method: null,
      cta: "Use this name",
    });
  });

  it("finds no workflows to fix when the search found none, and says what the search did when there was none", () => {
    const steps = [step("normal", CONV, "keep"), step("prod", FP16, "rename")];
    const none = unifyViewOf(plan(steps, [usage(FP16, [])]), installs);
    expect(none.kind === "confirm" && none.workflows).toEqual([]);
    const unsearched = unifyViewOf(plan(steps, [usage(FP16, [], false)]), installs);
    expect(unsearched.kind === "confirm" && unsearched.workflows).toBeNull();
    expect(unsearched.kind === "confirm" && unsearched.method).toBe(
      "There were no saved workflow files to search.",
    );
  });

  it("asks to close a running ComfyUI first", () => {
    const view = unifyViewOf(plan([step("normal", FP16, "rename")], [], ["normal"]), installs);
    expect(view).toEqual({
      kind: "running",
      heading: "Close ComfyUI-Beta first",
      body: "The name can change after ComfyUI-Beta is closed.",
    });
  });

  it("names only the install that changes when the name is taken in another", () => {
    const view = unifyViewOf(
      plan(
        [step("normal", FP16, "rename"), step("prod", FP16, "blockedTaken")],
        [usage(FP16, [["normal", "upscale_3d_scene.json"], ["prod", "h3_batch.json"]])],
      ),
      installs,
    );
    expect(view).toEqual({
      kind: "confirm",
      heading: [{ text: "Use this name in " }, { text: "ComfyUI-Beta", strong: true }, { text: "?" }],
      name: CONV,
      taken: ["ComfyUI-Alpha"],
      goingAway: [FP16],
      inInstalls: "ComfyUI-Beta",
      workflows: ["upscale_3d_scene.json"],
      method: null,
      cta: "Change name in ComfyUI-Beta",
    });
    expect(takenLines(["ComfyUI-Alpha"])).toEqual([
      "ComfyUI-Alpha already uses this name for another file.",
      "Its model name will stay as it is.",
    ]);
  });
});

describe("Cleanup's line once the name changed", () => {
  const renamed = step("normal", FP16, "rename");
  it("says both installs when both changed", () => {
    expect(
      unifyResultLine(
        { renamed: [renamed], removed: [step("prod", FP16, "remove")], skipped: [], stopped: null },
        installs,
      ),
    ).toBe("Name changed in both installs.");
  });

  it("says which install kept its name", () => {
    expect(
      unifyResultLine(
        {
          renamed: [renamed],
          removed: [],
          skipped: [{ step: step("prod", FP16, "blockedTaken"), reason: "taken" }],
          stopped: null,
        },
        installs,
      ),
    ).toBe("Name changed in ComfyUI-Beta. ComfyUI-Alpha kept its name.");
  });
});
