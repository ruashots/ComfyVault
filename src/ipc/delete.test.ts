import { describe, expect, it } from "vitest";

import { FixtureEngine, uniqueDefaultLabel } from "~/ipc/fixture/engine";
import type { VaultError, VaultFile } from "~/ipc/contract";

/**
 * The development engine must answer a delete exactly as section 8.8 of the
 * contract says the real one does, or the screens are built against a promise
 * the desktop app does not keep.
 */

/** Apply every group of the sample plan, and hand back a model with links. */
async function afterARun(): Promise<{ engine: FixtureEngine; model: VaultFile }> {
  const engine = new FixtureEngine({ manual: true });
  engine.devSetSymlinksSupported(true);
  engine.devSetComfyRunning(false);
  const scan = (await engine.getLastScan())!;
  const plan = await engine.buildPlan(scan.scanId);
  await engine.startApply({ planId: plan.planId, groupIds: plan.groups.map((g) => g.groupId) });
  engine.devFinish();
  const files = (await engine.listVaultFiles({ offset: 0, limit: 1000 })).files;
  const model = files.find((f) => f.linkCount >= 2)!;
  return { engine, model };
}

const refusal = async (p: Promise<unknown>): Promise<VaultError> => {
  try {
    await p;
  } catch (e) {
    return e as VaultError;
  }
  throw new Error("the engine did not refuse");
};

describe("deleting a model and every link to it", () => {
  it("removes the file, every link and every record of them", async () => {
    const { engine, model } = await afterARun();
    const links = await engine.listLinks({ sha256: model.sha256 });
    expect(links.length).toBeGreaterThanOrEqual(2);
    const freeBefore = (await engine.getVaultInfo()).freeBytes!;

    const done = await engine.deleteVaultFile(model.sha256, model.sha256, true);

    expect(done.deleted).toBe(true);
    expect(done.bytesFreed).toBe(model.sizeBytes);
    expect([...done.linksRemoved].sort()).toEqual(links.map((l) => l.absPath).sort());
    expect(await engine.listLinks({ sha256: model.sha256 })).toEqual([]);
    const files = (await engine.listVaultFiles({ offset: 0, limit: 1000 })).files;
    expect(files.some((f) => f.sha256 === model.sha256)).toBe(false);
    // It is gone, so it is not left behind as a file nothing links to.
    expect((await engine.listOrphans()).some((f) => f.sha256 === model.sha256)).toBe(false);
    // And nothing points at nothing afterwards.
    expect((await engine.checkVaultHealth()).danglingLinks).toEqual([]);
    expect((await engine.getVaultInfo()).freeBytes).toBe(freeBefore + model.sizeBytes);
  });

  it("still refuses a linked file when the links are not to go", async () => {
    const { engine, model } = await afterARun();
    const e = await refusal(engine.deleteVaultFile(model.sha256, model.sha256));
    expect(e.code).toBe("conflict");
    expect(e.message).toBe(
      "Some installs still link to that model, so it was kept. Remove those links first.",
    );
    const links = await engine.listLinks({ sha256: model.sha256 });
    expect(e.detail).toBe(links.map((l) => l.absPath).join(", "));
    expect((await engine.listLinks({ sha256: model.sha256 })).length).toBe(model.linkCount);
  });

  it("returns no links for a file nothing linked to", async () => {
    const { engine } = await afterARun();
    const orphan = (await engine.listOrphans())[0]!;
    const done = await engine.deleteVaultFile(orphan.sha256, orphan.sha256);
    expect(done.linksRemoved).toEqual([]);
  });

  it("removes nothing when a link path now holds a real file, and names that path", async () => {
    const { engine, model } = await afterARun();
    const links = await engine.listLinks({ sha256: model.sha256 });
    const replaced = links[1]!.absPath;
    engine.devReplaceLink(replaced);

    const e = await refusal(engine.deleteVaultFile(model.sha256, model.sha256, true));

    expect(e.code).toBe("conflict");
    expect(e.message).toBe(
      "Some of this model's links are not links to it any more, so nothing was deleted. Something else sits at these paths now.",
    );
    expect(e.detail).toBe(replaced);
    expect(e.path).toBe(replaced);
    expect((await engine.listLinks({ sha256: model.sha256 })).length).toBe(links.length);
    const files = (await engine.listVaultFiles({ offset: 0, limit: 1000 })).files;
    expect(files.some((f) => f.sha256 === model.sha256)).toBe(true);
  });

  it("removes nothing while another program holds the file open, and names the file", async () => {
    const { engine, model } = await afterARun();
    engine.devHoldOpen(model.sha256);

    const e = await refusal(engine.deleteVaultFile(model.sha256, model.sha256, true));

    expect(e.code).toBe("fileLocked");
    expect(e.path).toBe(`C:\\ComfyVault\\${model.vaultRelPath}`);
    expect(e.detail).toBe(e.path);
    expect((await engine.listLinks({ sha256: model.sha256 })).length).toBe(model.linkCount);
  });

  it("refuses a delete that was not confirmed, and one the vault does not hold", async () => {
    const { engine, model } = await afterARun();
    for (const removeLinks of [true, false]) {
      const e = await refusal(engine.deleteVaultFile(model.sha256, "nope", removeLinks));
      expect(e.code).toBe("invalidArgument");
      expect(e.message).toBe("This delete was not confirmed, so nothing was removed.");
    }
    expect((await refusal(engine.deleteVaultFile("F".repeat(64), "F".repeat(64), true))).code).toBe(
      "notFound",
    );
  });
});

describe("the name a new install gets when the person gives none", () => {
  it("is the folder the person picked, not the ComfyUI folder inside it", () => {
    expect(
      uniqueDefaultLabel("C:\\AI\\ComfyUI-Easy-Install", "C:\\AI\\ComfyUI-Easy-Install\\ComfyUI", []),
    ).toBe("ComfyUI-Easy-Install");
  });

  it("moves up a folder when another install has that name, whatever its case", () => {
    const root = "C:\\ComfyUI_windows_portable\\ComfyUI";
    expect(uniqueDefaultLabel(root, root, ["comfyui"])).toBe("ComfyUI_windows_portable");
    expect(uniqueDefaultLabel(root, root, [])).toBe("ComfyUI");
  });

  it("is the whole root when every folder name is taken", () => {
    expect(uniqueDefaultLabel("C:\\A\\ComfyUI", "C:\\A\\ComfyUI", ["ComfyUI", "A"])).toBe(
      "C:\\A\\ComfyUI",
    );
  });
});

describe("undoing a run after one of its models was deleted", () => {
  it("is refused with the reason, in the preview and the undo, and nothing changes", async () => {
    const { engine, model } = await afterARun();
    const done = await engine.deleteVaultFile(model.sha256, model.sha256, true);
    const applyId = (await engine.listApplies())[0]!.applyId;
    const message =
      "One of this run's models was deleted in Cleanup, so this run can no longer be undone. Nothing was changed.";
    const expected = [`C:\\ComfyVault\\${model.vaultRelPath}`, ...done.linksRemoved].sort().join(", ");

    for (const e of [
      await refusal(engine.previewRevert(applyId)),
      await refusal(engine.revertApply(applyId)),
    ]) {
      expect(e.code).toBe("conflict");
      expect(e.message).toBe(message);
      expect(e.detail).toBe(expected);
    }
    expect((await engine.listApplies())[0]!.state).not.toBe("partlyReverted");
  });
});
