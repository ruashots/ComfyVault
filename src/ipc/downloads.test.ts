import { describe, expect, it } from "vitest";

import { FixtureEngine } from "~/ipc/fixture/engine";
import { parseAddress } from "~/ipc/fixture/downloads";
import type { Download, VaultError } from "~/ipc/contract";

/**
 * The development engine's downloader must answer the way the real engine
 * does, or the Download screen is built against a promise the desktop app does
 * not keep.
 */

const DREAM = "https://civitai.com/models/4384/dreamshaper";
const FLUX_FP8 = "https://huggingface.co/Comfy-Org/flux1-dev/blob/main/flux1-dev-fp8.safetensors";
const GATED = "https://huggingface.co/black-forest-labs/FLUX.1-dev/blob/main/flux1-dev.safetensors";
const T5 = "https://huggingface.co/comfyanonymous/flux_text_encoders/blob/main/t5xxl_fp16.safetensors";
const WAN_VAE =
  "https://huggingface.co/Comfy-Org/Wan_2.2_ComfyUI_Repackaged/blob/main/split_files/vae/wan2.2_vae.safetensors";

const refusal = async (p: Promise<unknown>): Promise<VaultError> => {
  try {
    await p;
  } catch (e) {
    return e as VaultError;
  }
  throw new Error("the engine did not refuse");
};

function engine() {
  const e = new FixtureEngine({ manual: true });
  const seen: Download[] = [];
  e.onDownloadProgress((r) => seen.push(r));
  return { e, seen };
}

/** After a run, so the vault holds the sample models. */
async function afterARun() {
  const { e, seen } = engine();
  e.devSetSymlinksSupported(true);
  e.devSetComfyRunning(false);
  const scan = (await e.getLastScan())!;
  const plan = await e.buildPlan(scan.scanId);
  await e.startApply({ planId: plan.planId, groupIds: plan.groups.map((g) => g.groupId) });
  e.devFinish();
  return { e, seen };
}

describe("the addresses the engine reads", () => {
  it("reads every Hugging Face file form", () => {
    const want = {
      host: "huggingface",
      owner: "Comfy-Org",
      repo: "flux1-dev",
      revision: "main",
      path: "flux1-dev-fp8.safetensors",
    };
    for (const a of [
      FLUX_FP8,
      "https://huggingface.co/Comfy-Org/flux1-dev/resolve/main/flux1-dev-fp8.safetensors",
      "https://huggingface.co/Comfy-Org/flux1-dev/resolve/main/flux1-dev-fp8.safetensors?download=true",
      "huggingface.co/Comfy-Org/flux1-dev/blob/main/flux1-dev-fp8.safetensors",
    ]) {
      expect(parseAddress(a), a).toEqual(want);
    }
  });

  it("reads every Civitai form", () => {
    expect(parseAddress("https://civitai.com/models/4384")).toEqual({ host: "civitai", modelId: 4384, versionId: null });
    expect(parseAddress(DREAM)).toEqual({ host: "civitai", modelId: 4384, versionId: null });
    expect(parseAddress("https://civitai.com/models/4384/dreamshaper?modelVersionId=128713")).toEqual({
      host: "civitai",
      modelId: 4384,
      versionId: 128713,
    });
    expect(parseAddress("https://civitai.com/api/download/models/128713")).toEqual({
      host: "civitai",
      modelId: null,
      versionId: 128713,
    });
  });

  it("tells an address from elsewhere apart from a Hugging Face model page", () => {
    expect(parseAddress("https://drive.google.com/file/d/1aXf93/view")).toBe("bad");
    expect(parseAddress("https://huggingface.co/Comfy-Org/flux1-dev")).toBe("hfRepoNotFile");
    expect(parseAddress("https://huggingface.co/Comfy-Org/flux1-dev/tree/main")).toBe("hfRepoNotFile");
    expect(parseAddress("https://huggingface.co/datasets/a/b/blob/main/c")).toBe("bad");
    expect(parseAddress("https://huggingface.co/a/b/blob/main/..%2F..%2Fx")).toBe("bad");
  });
});

describe("reading an address into a plan", () => {
  it("gives the refusal inside the plan for an address it cannot use", async () => {
    const { e } = engine();
    expect((await e.readModelAddress({ address: "https://drive.google.com/x" })).refusal).toEqual({
      kind: "badAddress",
      host: null,
      title: null,
      subtitle: null,
      serviceMessage: null,
      page: null,
    });
    expect((await e.readModelAddress({ address: "https://huggingface.co/Comfy-Org/flux1-dev" })).refusal!.kind).toBe(
      "hfRepoNotFile",
    );
  });

  it("chooses the newest Civitai version and its primary file, and the menus change it", async () => {
    const { e } = engine();
    const plan = (await e.readModelAddress({ address: DREAM })).plan!;
    expect(plan.title).toBe("DreamShaper");
    expect(plan.versions.map((v) => v.name)).toEqual(["8", "8 LCM", "8-inpainting", "7"]);
    expect(plan.versionId).toBe(plan.versions[0]!.id);
    expect(plan.fileName).toBe("dreamshaper_8.safetensors");
    expect(plan.sha256).toBe("879DB523C30D3B9017143D56705015E15A2CB5628762C11D086FED9538ABD7FD");
    expect(plan.category).toBe("checkpoints");
    expect(plan.suggestedBecause).toBe("Civitai calls it a Checkpoint");
    expect(plan.vaultRelPath).toBe("checkpoints\\dreamshaper_8.safetensors");

    const seven = (await e.readModelAddress({ address: DREAM, versionId: plan.versions[3]!.id })).plan!;
    expect(seven.fileName).toBe("dreamshaper_7.safetensors");
    const full = (await e.readModelAddress({
      address: DREAM,
      versionId: seven.versionId!,
      fileId: seven.files[1]!.id,
    })).plan!;
    expect(full.fileName).toBe("dreamshaper_7-full.safetensors");
  });

  it("suggests no folder for a Hugging Face file whose path gives no hint", async () => {
    const { e } = engine();
    const plan = (await e.readModelAddress({ address: FLUX_FP8 })).plan!;
    expect(plan.category).toBeNull();
    expect(plan.suggestedCategory).toBeNull();
    expect(plan.vaultRelPath).toBeNull();
    expect(plan.installs.every((i) => i.linkPath === null)).toBe(true);
    const chosen = (await e.readModelAddress({ address: FLUX_FP8, category: "diffusion_models" })).plan!;
    expect(chosen.vaultRelPath).toBe("diffusion_models\\flux1-dev-fp8.safetensors");
    expect(chosen.installs.every((i) => i.linkPath!.endsWith("\\models\\diffusion_models\\flux1-dev-fp8.safetensors"))).toBe(
      true,
    );
  });

  it("suggests the folder a Hugging Face path names", async () => {
    const { e } = engine();
    const plan = (await e.readModelAddress({ address: WAN_VAE })).plan!;
    expect(plan.category).toBe("vae");
    expect(plan.title).toBe("wan2.2_vae.safetensors");
    expect(plan.subtitle).toBe("Comfy-Org/Wan_2.2_ComfyUI_Repackaged");
  });

  it("says the vault already holds a file with the same SHA-256", async () => {
    const { e } = await afterARun();
    const plan = (await e.readModelAddress({ address: T5 })).plan!;
    expect(plan.alreadyInVault).toEqual({ vaultRelPath: "text_encoders\\t5xxl_fp16.safetensors" });
    expect(plan.installs.every((i) => i.state === "hasLink")).toBe(true);
  });

  it("holds an install whose folder has a different file with that name", async () => {
    const { e } = engine();
    e.downloads.devPlaceFile("sandbox", "diffusion_models", "flux1-dev-fp8.safetensors");
    const plan = (await e.readModelAddress({ address: FLUX_FP8, category: "diffusion_models" })).plan!;
    expect(plan.installs.find((i) => i.installId === "sandbox")!.state).toBe("nameTaken");
    expect(plan.installs.find((i) => i.installId === "studio")!.state).toBe("free");
  });

  it("finds each refusal while reading, with the service's own words", async () => {
    const { e } = engine();
    const missing = await e.readModelAddress({ address: GATED });
    expect(missing.plan).toBeNull();
    expect(missing.refusal).toEqual({
      kind: "tokenMissing",
      host: "huggingface",
      title: "flux1-dev.safetensors",
      subtitle: "black-forest-labs/FLUX.1-dev",
      serviceMessage:
        "Access to model black-forest-labs/FLUX.1-dev is restricted. You must have access to it and be authenticated to access it. Please log in.",
      page: { owner: "black-forest-labs", repo: "FLUX.1-dev" },
    });
    await e.setToken("huggingface", "hf_good");
    expect((await e.readModelAddress({ address: GATED })).refusal!.kind).toBe("noAccess");
    e.downloads.devAcceptTerms("black-forest-labs", "FLUX.1-dev");
    expect((await e.readModelAddress({ address: GATED })).refusal).toBeNull();
    e.downloads.devRevokeToken("huggingface");
    const rejected = (await e.readModelAddress({ address: GATED })).refusal!;
    expect([rejected.kind, rejected.serviceMessage]).toEqual(["tokenRejected", "Invalid username or password."]);
    const civitai = (await e.readModelAddress({ address: "https://civitai.com/models/123456" })).refusal!;
    expect([civitai.kind, civitai.host, civitai.serviceMessage]).toEqual([
      "tokenMissing",
      "civitai",
      "The creator of this asset requires you to be logged in to download it",
    ]);
  });
});

describe("the tokens", () => {
  it("keeps a token only once the service accepts it, and never gives it back", async () => {
    const { e } = engine();
    expect(await e.getTokenStatus("huggingface")).toEqual({ saved: false, ok: null, account: null, message: null });
    const refused = await refusal(e.setToken("huggingface", "hf_bad"));
    expect(refused.detail).toBe("Invalid username or password.");
    expect((await e.getTokenStatus("huggingface")).saved).toBe(false);

    expect(await e.setToken("huggingface", "hf_good")).toEqual({ ok: true, account: "example-user" });
    const status = await e.getTokenStatus("huggingface");
    expect(status).toEqual({ saved: true, ok: true, account: "example-user", message: null });
    expect(JSON.stringify(status)).not.toContain("hf_good");

    await e.removeToken("huggingface");
    expect((await e.getTokenStatus("huggingface")).saved).toBe(false);
  });
});

describe("the queue", () => {
  it("runs one download at a time, in order, and reports every change", async () => {
    const { e, seen } = engine();
    const a = await e.startDownload({ address: DREAM, category: "checkpoints", installIds: ["studio"] });
    const b = await e.startDownload({ address: WAN_VAE, category: "vae", installIds: [] });
    const list = await e.listDownloads();
    expect(list.map((r) => r.state)).toEqual(["running", "waiting"]);
    expect(a.state).toBe("running");
    expect(b.state).toBe("waiting");

    e.downloads.devFinishDownloads();
    const done = await e.listDownloads();
    expect(done.map((r) => r.state)).toEqual(["done", "done"]);
    // The waiting one was reported when it started, not only when it ended.
    expect(seen.some((r) => r.downloadId === b.downloadId && r.state === "running")).toBe(true);
    const dream = done[0]!;
    expect(dream.sha256).toBe("879DB523C30D3B9017143D56705015E15A2CB5628762C11D086FED9538ABD7FD");
    expect(dream.linkedInstallIds).toEqual(["studio"]);
    const files = (await e.listVaultFiles({ offset: 0, limit: 1000 })).files;
    expect(
      files.some(
        (f) =>
          f.vaultRelPath === dream.vaultRelPath &&
          f.sha256 === "879DB523C30D3B9017143D56705015E15A2CB5628762C11D086FED9538ABD7FD" &&
          f.linkCount === 1,
      ),
    ).toBe(true);
  });

  it("continues a stopped download from the part it kept", async () => {
    const { e } = engine();
    const r = await e.startDownload({ address: DREAM, category: "checkpoints", installIds: [] });
    for (let i = 0; i < 20; i++) e.downloads.devStep();
    const stopped = await e.stopDownload(r.downloadId);
    expect(stopped.state).toBe("stopped");
    const kept = stopped.bytesDone;
    expect(kept).toBeGreaterThan(0);
    const again = await e.continueDownload(r.downloadId);
    expect(again.state).toBe("running");
    expect(again.bytesDone).toBe(kept);
    e.downloads.devFinishDownloads();
    const [final] = await e.listDownloads();
    expect(final!.state).toBe("done");
    const files = (await e.listVaultFiles({ offset: 0, limit: 1000 })).files;
    expect(files.find((f) => f.vaultRelPath === final!.vaultRelPath)!.sha256).toBe(
      "879DB523C30D3B9017143D56705015E15A2CB5628762C11D086FED9538ABD7FD",
    );
  });

  it("keeps a download cut off by a closed app, and continues it", async () => {
    const { e } = engine();
    const r = await e.startDownload({ address: DREAM, category: "checkpoints", installIds: [] });
    for (let i = 0; i < 10; i++) e.downloads.devStep();
    e.downloads.devCutOff();
    const [cut] = await e.listDownloads();
    expect(cut!.state).toBe("cutOff");
    expect(cut!.bytesDone).toBeGreaterThan(0);
    await e.continueDownload(r.downloadId);
    e.downloads.devFinishDownloads();
    expect((await e.listDownloads())[0]!.state).toBe("done");
  });

  it("deletes a file that does not match, links nothing, and can start again", async () => {
    const { e } = engine();
    e.downloads.devCorruptNext();
    const r = await e.startDownload({ address: DREAM, category: "checkpoints", installIds: ["studio"] });
    e.downloads.devFinishDownloads();
    const [bad] = await e.listDownloads();
    expect(bad!.state).toBe("mismatch");
    expect(bad!.sha256).toBeNull();
    expect(bad!.linkedInstallIds).toEqual([]);
    const files = (await e.listVaultFiles({ offset: 0, limit: 1000 })).files;
    expect(files.some((f) => f.canonicalName === "dreamshaper_8.safetensors")).toBe(false);
    const again = await e.continueDownload(r.downloadId);
    expect(again.bytesDone).toBe(0);
    e.downloads.devFinishDownloads();
    expect((await e.listDownloads())[0]!.state).toBe("done");
  });

  it("makes only the links when the vault already holds the file", async () => {
    const { e } = await afterARun();
    const r = await e.startDownload({ address: T5, category: "text_encoders", installIds: ["studio"] });
    // Every install already links it, so there is nothing to link and nothing to transfer.
    expect(r.state).toBe("linkedOnly");
    expect(r.alreadyInVault).toBe(true);
    expect(r.bytesDone).toBe(0);
  });

  it("reports a dropped connection and a refusal in the middle as failed, with the words", async () => {
    const { e } = engine();
    await e.startDownload({ address: DREAM, category: "checkpoints", installIds: [] });
    e.downloads.devStep();
    e.downloads.devDropConnection();
    expect((await e.listDownloads())[0]!.error!.kind).toBe("connection");
    await e.continueDownload((await e.listDownloads())[0]!.downloadId);
    e.downloads.devRefuseMidway("Forbidden");
    expect((await e.listDownloads())[0]!.error).toMatchObject({ kind: "refused", serviceMessage: "Forbidden" });
  });

  it("refuses a download without a folder, and one the drive has no room for", async () => {
    const { e } = engine();
    expect((await refusal(e.startDownload({ address: FLUX_FP8, category: "", installIds: [] }))).code).toBe(
      "invalidArgument",
    );
    e.downloads.devSetFreeBytes(12 * 1024 ** 3);
    const full = await refusal(
      e.startDownload({ address: FLUX_FP8, category: "diffusion_models", installIds: [] }),
    );
    expect(full.message).toBe(
      "There is not enough free space on the vault's drive for this file and the 5 GB kept free.",
    );
    expect(await e.listDownloads()).toEqual([]);
  });

  it("discards a kept part, and only takes finished or waiting rows off the list", async () => {
    const { e } = engine();
    const r = await e.startDownload({ address: DREAM, category: "checkpoints", installIds: [] });
    expect((await refusal(e.discardDownload(r.downloadId))).code).toBe("conflict");
    await e.stopDownload(r.downloadId);
    expect((await refusal(e.removeDownload(r.downloadId))).code).toBe("conflict");
    await e.discardDownload(r.downloadId);
    expect(await e.listDownloads()).toEqual([]);
  });
});

describe("what the engine keeps for the person", () => {
  it("ticks the installs ticked last time on the next plan", async () => {
    const { e } = engine();
    const first = (await e.readModelAddress({ address: DREAM })).plan!;
    expect(first.installs.filter((i) => i.ticked).map((i) => i.installId)).toEqual(["studio", "sandbox"]);
    await e.startDownload({ address: DREAM, category: "checkpoints", installIds: ["sandbox"] });
    const next = (await e.readModelAddress({ address: "https://civitai.com/models/58390" })).plan!;
    expect(next.installs.filter((i) => i.ticked).map((i) => i.installId)).toEqual(["sandbox"]);
  });

  it("asks for the file's size plus the margin free, and nothing for a file it holds", async () => {
    const { e } = engine();
    const plan = (await e.readModelAddress({ address: DREAM })).plan!;
    expect(plan.spaceNeededBytes).toBe(plan.sizeBytes + 5_000_000_000);
  });

  it("refuses a token the site does not accept with the site's words as the detail", async () => {
    const { e } = engine();
    const refused = await refusal(e.setToken("civitai", "bad"));
    expect(refused.code).toBe("conflict");
    expect(refused.detail).toBe("Invalid API key");
  });
});
