import { describe, expect, it } from "vitest";

import {
  activeCount,
  cutOffLine,
  cutOffSentence,
  hasRoom,
  hostOf,
  linkHint,
  listCount,
  listOrder,
  rowView,
  speedOf,
} from "~/domain/download";
import type { Install } from "~/ipc/contract";
import type { Download } from "~/ipc/draft";

const GB = 1024 ** 3;
const MB = 1024 ** 2;

const install = (id: string, label: string, root: string): Install => ({
  id,
  label,
  registeredPath: root,
  root,
  modelsDir: `${root}\\models`,
  version: null,
  versionSource: null,
  extraPaths: [],
  outputModelDirs: [],
  addedAt: "2026-09-26T00:00:00Z",
  lastScanAt: null,
  lastScanTotals: null,
});

const installs = [
  install("a", "ComfyUI", "C:\\AI\\ComfyUI-Easy-Install\\ComfyUI"),
  install("b", "ComfyUI", "C:\\ComfyUI_windows_portable\\ComfyUI"),
  install("c", "ComfyUI", "C:\\AI\\ComfyUI-Flux\\ComfyUI"),
];

const record = (over: Partial<Download> = {}): Download => ({
  downloadId: "d1",
  host: "huggingface",
  title: "flux1-dev-fp8.safetensors",
  fileName: "flux1-dev-fp8.safetensors",
  bytesTotal: 16 * GB,
  bytesDone: 6.2 * GB,
  bytesPerSecond: 38 * MB,
  state: "running",
  category: "diffusion_models",
  vaultRelPath: "diffusion_models\\flux1-dev-fp8.safetensors",
  sha256: null,
  installIds: ["a", "c"],
  linkedInstallIds: [],
  notLinked: [],
  alreadyInVault: false,
  error: null,
  startedAt: "2026-09-26T10:00:00Z",
  finishedAt: null,
  ...over,
});

const say = (r: Download) =>
  rowView(r, installs, "C:").parts.map((p) => p.text).join("");

describe("what a row of the Downloads list says", () => {
  it("says what is happening while it runs, with speed and time left", () => {
    const view = rowView(record(), installs, "C:");
    expect(say(record())).toBe(
      "Downloading: 6.2 GB of 16 GB, 38 MB/s, about 4 minutes left.",
    );
    expect(view.parts[0]).toEqual({ text: "Downloading:", tone: "now" });
    expect(view.bar).toEqual({ fraction: 6.2 / 16, stopped: false });
    expect(view.actions).toEqual(["stop"]);
  });

  it("says it is working before the speed is known", () => {
    expect(say(record({ bytesPerSecond: null, bytesDone: 0 }))).toBe(
      "Downloading: 0 MB of 16 GB, working.",
    );
  });

  it("says a waiting one will start after the one above, and where it will be linked", () => {
    const r = record({ state: "waiting", installIds: ["a", "b"] });
    expect(say(r)).toBe(
      "Will start when the download above is done. Then it will be linked in ComfyUI-Easy-Install and ComfyUI_windows_portable.",
    );
    expect(rowView(r, installs, "C:").actions).toEqual(["remove"]);
    expect(rowView(r, installs, "C:").bar).toBeNull();
  });

  it("says a check comes before the vault and the links", () => {
    const view = rowView(record({ state: "checking" }), installs, "C:");
    expect(say(record({ state: "checking" }))).toBe(
      "Checking the SHA-256 of the downloaded file. Then it goes into the vault and gets its links.",
    );
    expect(view.bar).toEqual({ fraction: 1, stopped: false });
    expect(view.actions).toEqual([]);
  });

  it("keeps the part of a stopped, dropped or cut-off download, and offers to continue", () => {
    expect(say(record({ state: "stopped" }))).toBe(
      "Stopped at 6.2 GB of 16 GB. The part already downloaded is kept, so it can continue from there.",
    );
    const dropped = record({ state: "failed", error: { kind: "connection", message: "The connection dropped.", serviceMessage: null } });
    expect(say(dropped)).toBe(
      "The connection to Hugging Face dropped at 6.2 GB of 16 GB. The part already downloaded is kept.",
    );
    expect(rowView(dropped, installs, "C:").parts[0]!.tone).toBe("bad");
    expect(say(record({ state: "cutOff" }))).toBe(
      "Cut off at 6.2 GB of 16 GB when ComfyVault closed. The part already downloaded is kept.",
    );
    for (const state of ["stopped", "failed", "cutOff"] as const) {
      const view = rowView(record({ state }), installs, "C:");
      expect(view.actions).toEqual(["continue", "discard"]);
      expect(view.bar!.stopped).toBe(true);
    }
  });

  it("passes the service's own words through when it refuses in the middle", () => {
    const r = record({
      state: "failed",
      error: {
        kind: "refused",
        message: "The site refused the download.",
        serviceMessage: "Access to model x is restricted.",
      },
    });
    expect(say(r)).toBe(
      'Hugging Face stopped the download at 6.2 GB of 16 GB. It says: "Access to model x is restricted." The part already downloaded is kept.',
    );
  });

  it("says the vault drive filled up, and what to do", () => {
    const r = record({
      state: "failed",
      error: { kind: "noSpace", message: "The vault drive is full.", serviceMessage: null },
    });
    expect(say(r)).toBe(
      "Drive C: ran out of space at 6.2 GB of 16 GB. The part already downloaded is kept. Free some space, then continue.",
    );
  });

  it("says any other stop in the engine's own sentence", () => {
    const disk = record({
      state: "failed",
      error: { kind: "disk", message: "Windows could not write the part file.", serviceMessage: null },
    });
    expect(say(disk)).toBe(
      "Windows could not write the part file. The part already downloaded is kept.",
    );
    const changed = record({
      state: "failed",
      error: {
        kind: "changedOnSite",
        message: "The file on Hugging Face changed since this download started.",
        serviceMessage: null,
      },
    });
    expect(say(changed)).toBe("The file on Hugging Face changed since this download started.");
  });

  it("says a file that did not match was deleted and nothing was linked", () => {
    const r = record({ state: "mismatch", host: "civitai", bytesDone: 0 });
    expect(say(r)).toBe(
      "The downloaded file did not match the SHA-256 Civitai gave, so it was deleted. Nothing went into the vault and nothing was linked. This happens when the file changed on Civitai or the transfer was damaged.",
    );
    expect(rowView(r, installs, "C:").actions).toEqual(["again", "remove"]);
  });

  it("says what happened when it is done", () => {
    const r = record({
      state: "done",
      vaultRelPath: "checkpoints\\dreamshaper_8.safetensors",
      linkedInstallIds: ["a", "b"],
    });
    expect(say(r)).toBe(
      "Downloaded into the vault as checkpoints\\dreamshaper_8.safetensors, and linked in ComfyUI-Easy-Install and ComfyUI_windows_portable.",
    );
    expect(rowView(r, installs, "C:").parts[0]).toEqual({ text: "Downloaded", tone: "ok" });
    expect(rowView(r, installs, "C:").actions).toEqual(["library"]);
    expect(say(record({ state: "done", linkedInstallIds: [] }))).toBe(
      "Downloaded into the vault as diffusion_models\\flux1-dev-fp8.safetensors.",
    );
  });

  it("says nothing was downloaded when the vault already had the file", () => {
    expect(say(record({ state: "linkedOnly", alreadyInVault: true, linkedInstallIds: ["c"] }))).toBe(
      "Linked in ComfyUI-Flux. It was already in the vault, so nothing was downloaded.",
    );
    // A Hugging Face file with no hash up front, found in the vault after it came.
    expect(say(record({ state: "done", alreadyInVault: true, linkedInstallIds: ["c"] }))).toBe(
      "Linked in ComfyUI-Flux. It was already in the vault, so the new file was deleted.",
    );
  });

  it("says which install could not get its link at the end, in the engine's words", () => {
    const r = record({
      state: "done",
      vaultRelPath: "loras\\x.safetensors",
      linkedInstallIds: ["a"],
      notLinked: [{ installId: "c", reason: "A file with that name appeared there in the meantime." }],
    });
    const view = rowView(r, installs, "C:");
    expect(say(r)).toBe(
      "Downloaded into the vault as loras\\x.safetensors, and linked in ComfyUI-Easy-Install. It was not linked in ComfyUI-Flux: A file with that name appeared there in the meantime.",
    );
    expect(view.parts.at(-1)!.tone).toBe("bad");
  });
});

describe("the counts around the list", () => {
  const rows = [
    record({ downloadId: "1", state: "running" }),
    record({ downloadId: "2", state: "waiting" }),
    record({ downloadId: "3", state: "done" }),
    record({ downloadId: "4", state: "stopped" }),
  ];

  it("says how many are not finished, in a sentence", () => {
    expect(listCount(rows)).toBe("3 are not finished");
    expect(listCount(rows.slice(2, 3))).toBe("all finished");
    expect(listCount(rows.slice(0, 1))).toBe("1 is not finished");
  });

  it("puts what is not finished first, in order, and the finished ones after, newest first", () => {
    const order = listOrder([
      record({ downloadId: "old-done", state: "done" }),
      record({ downloadId: "running", state: "running" }),
      record({ downloadId: "new-done", state: "linkedOnly" }),
      record({ downloadId: "waiting", state: "waiting" }),
    ]).map((r) => r.downloadId);
    expect(order).toEqual(["running", "waiting", "new-done", "old-done"]);
  });

  it("counts the ones under way or waiting for the rail", () => {
    expect(activeCount(rows)).toBe(2);
    expect(activeCount([record({ state: "checking" })])).toBe(1);
  });
});

describe("a download ComfyVault closed on", () => {
  it("is said once in the banner and once on Home", () => {
    const cut = [record({ state: "cutOff" })];
    expect(cutOffSentence(cut)).toBe(
      "ComfyVault closed while flux1-dev-fp8.safetensors was downloading. 6.2 GB of 16 GB is kept. Continue it from there, or discard it.",
    );
    expect(cutOffLine(cut)).toEqual({
      text: "flux1-dev-fp8.safetensors was cut off at 6.2 GB of 16 GB when ComfyVault closed.",
      link: "Continue it or discard it",
    });
    expect(cutOffSentence([record({ state: "stopped" })])).toBeNull();
    expect(cutOffLine([])).toBeNull();
  });
});

describe("the plan card's rules", () => {
  it("needs the file size plus the margin free on the vault drive", () => {
    expect(hasRoom({ spaceNeededBytes: 21 * GB, vaultFreeBytes: 21 * GB })).toBe(true);
    expect(hasRoom({ spaceNeededBytes: 21 * GB, vaultFreeBytes: 21 * GB - 1 })).toBe(false);
    // A drive that did not answer is left to the engine to refuse.
    expect(hasRoom({ spaceNeededBytes: 21 * GB, vaultFreeBytes: null })).toBe(true);
  });

  it("says how many installs will get a link", () => {
    expect(linkHint(2)).toBe("Then it will be linked in 2 installs.");
    expect(linkHint(1)).toBe("Then it will be linked in 1 install.");
    expect(linkHint(0)).toBeNull();
  });

  it("knows which service a pasted address is for, and nothing more", () => {
    expect(hostOf("https://huggingface.co/a/b/blob/main/c")).toBe("huggingface");
    expect(hostOf("civitai.com/models/4384")).toBe("civitai");
    expect(hostOf("https://drive.google.com/file/d/1")).toBeNull();
    expect(hostOf("https://huggingface.co.evil.example/a")).toBeNull();
  });

  it("says the speed in MB/s once it is known", () => {
    expect(speedOf(38 * MB)).toBe("38 MB/s");
    expect(speedOf(2.5 * MB)).toBe("2.5 MB/s");
    expect(speedOf(null)).toBeNull();
    expect(speedOf(0)).toBeNull();
  });
});
