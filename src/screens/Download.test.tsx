import { fireEvent, screen } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";

import { App } from "~/App";
import { fmt } from "~/domain/format";
import { ConfirmModalView } from "~/modals/confirm";
import { DownloadScreen } from "~/screens/Download";
import { FixtureEngine } from "~/ipc/fixture/engine";
import { renderWithApp, waitFor, type Harness } from "~/test/render";

let harness: Harness | null = null;

afterEach(() => {
  harness?.unmount();
  harness = null;
});

const DREAM = "https://civitai.com/models/4384/dreamshaper";
const FLUX_FP8 = "https://huggingface.co/Comfy-Org/flux1-dev/blob/main/flux1-dev-fp8.safetensors";
const GATED = "https://huggingface.co/black-forest-labs/FLUX.1-dev/blob/main/flux1-dev.safetensors";
const T5 = "https://huggingface.co/comfyanonymous/flux_text_encoders/blob/main/t5xxl_fp16.safetensors";

async function mount(prepare?: (engine: FixtureEngine) => Promise<void> | void) {
  const engine = new FixtureEngine({ manual: true });
  await prepare?.(engine);
  harness = await renderWithApp(
    () => (
      <>
        <DownloadScreen />
        <ConfirmModalView />
      </>
    ),
    { engine },
  );
  return harness;
}

const text = () => (document.body.textContent ?? "").replace(/\s+/g, " ");
const field = () => screen.getByRole("textbox", { name: /The address of a model/ }) as HTMLInputElement;
const button = (name: string | RegExp) => screen.getByRole("button", { name });

async function read(address: string) {
  await userEvent.clear(field());
  await userEvent.type(field(), address);
  await userEvent.click(button("Read the address"));
  await waitFor(() => !harness!.app.dl.card.reading);
}

const card = () => document.querySelector(".plan-card");

describe("before a vault exists", () => {
  it("sends the person to set up first", async () => {
    harness = await renderWithApp(() => <DownloadScreen />, {
      engine: new FixtureEngine({ empty: true }),
    });
    expect(text()).toContain("A downloaded model goes into the vault");
    expect(screen.queryByRole("textbox")).toBeNull();
  });
});

describe("pasting an address", () => {
  it("offers Read only once there is text, and says what the field takes", async () => {
    await mount();
    expect(button("Read the address")).toBeDisabled();
    expect(text()).toContain(
      "A Hugging Face file address, as you get it from the file's page: huggingface.co/owner/model/blob/main/file.safetensors. Or a Civitai model page: civitai.com/models/4384, with or without a version.",
    );
    await userEvent.type(field(), "x");
    expect(button("Read the address")).toBeEnabled();
  });

  it("says an address from elsewhere cannot be used, in red, instead of the help", async () => {
    await mount();
    await read("https://drive.google.com/file/d/1aXf93/view");
    const err = document.querySelector(".paste-err")!;
    expect(err.textContent).toBe(
      "This is not a Hugging Face or Civitai address. ComfyVault downloads from huggingface.co and civitai.com only.",
    );
    expect(document.querySelector(".paste .field.bad")).not.toBeNull();
    expect(document.querySelector(".paste-help")).toBeNull();
    expect(card()).toBeNull();
  });

  it("says a Hugging Face model page is not a file, and where to find the file", async () => {
    await mount();
    await read("https://huggingface.co/Comfy-Org/flux1-dev");
    expect(document.querySelector(".paste-err")!.textContent).toBe(
      'This is the address of a whole model on Hugging Face, not of one file. Open the "Files and versions" tab, click the file you want, and copy that page\'s address.',
    );
  });

  it("reads at once when an address is pasted, and on Enter", async () => {
    const { app } = await mount();
    field().focus();
    fireEvent.paste(field(), { clipboardData: { getData: () => DREAM } });
    await waitFor(() => app.dl.card.plan !== null);
    expect(app.dl.card.address).toBe(DREAM);
    app.dl.cancel();
    await userEvent.clear(field());
    await userEvent.type(field(), `${FLUX_FP8}{Enter}`);
    await waitFor(() => app.dl.card.plan?.fileName === "flux1-dev-fp8.safetensors");
  });
});

describe("reading the address", () => {
  it("says it is asking the service, and Cancel drops the answer when it comes", async () => {
    let answer!: () => void;
    const { app, engine } = await mount((e) => {
      const original = e.readModelAddress.bind(e);
      e.readModelAddress = (args) =>
        new Promise((resolve) => {
          answer = () => void original(args).then(resolve);
        });
    });
    await userEvent.type(field(), DREAM);
    await userEvent.click(button("Read the address"));
    await waitFor(() => app.dl.card.reading);
    expect(card()!.querySelector("h4")!.textContent).toBe("Reading the address");
    expect(card()!.querySelector("p")!.textContent!.replace(/\s+/g, " ")).toBe(
      "Asking Civitai for the file, its size and its SHA-256. Nothing is downloaded yet.",
    );
    await userEvent.click(button("Cancel"));
    answer();
    await new Promise((r) => setTimeout(r, 20));
    expect(app.dl.card.plan).toBeNull();
    expect(card()).toBeNull();
    // The address stays for another try.
    expect(field().value).toBe(DREAM);
    void engine;
  });

  it("shows only the newest answer when two reads cross", async () => {
    const answers: (() => void)[] = [];
    const { app } = await mount((e) => {
      const original = e.readModelAddress.bind(e);
      e.readModelAddress = (args) =>
        new Promise((resolve) => {
          answers.push(() => void original(args).then(resolve));
        });
    });
    void app.dl.read(DREAM);
    void app.dl.read(FLUX_FP8);
    answers[1]!();
    await waitFor(() => app.dl.card.plan !== null);
    answers[0]!();
    await new Promise((r) => setTimeout(r, 20));
    expect(app.dl.card.plan!.fileName).toBe("flux1-dev-fp8.safetensors");
  });
});

describe("the plan for a Civitai model", () => {
  it("states what will happen, as a plan", async () => {
    await mount();
    await read(DREAM);
    expect(card()!.querySelector(".card-h")!.textContent).toBe("DreamShaper, version 8from Civitai");
    const rows = [...card()!.querySelectorAll(".kv")].map((kv) =>
      `${kv.querySelector(".k")!.textContent} ${kv.querySelector(".v")!.textContent}`.replace(/\s+/g, " "),
    );
    expect(rows[0]).toBe("Will download dreamshaper_8.safetensors, 2.0 GB");
    expect(rows[1]).toContain("Will go into the vault as checkpoints\\dreamshaper_8.safetensors (Civitai calls it a Checkpoint)");
    expect(rows[2]).toContain("Will be linked in");
    expect(rows[3]).toMatch(/^Will leave free on drive C: \S+ GB of the \S+ GB free now$/);
    expect(rows[4]).toBe(
      "Will be checked against SHA-256 879DB523…38ABD7FD from Civitai. A file that does not match is deleted.",
    );
    expect(button("Download 2.0 GB")).toBeEnabled();
    expect(text()).toContain("Then it will be linked in 2 installs.");
  });

  it("changes the plan when another version or file is chosen", async () => {
    const { app } = await mount();
    await read(DREAM);
    const version = screen.getByRole("combobox", { name: "Version" }) as HTMLSelectElement;
    await userEvent.selectOptions(version, "7");
    await waitFor(() => app.dl.card.plan?.fileName === "dreamshaper_7.safetensors");
    expect(card()!.querySelector(".card-h .nm")!.textContent).toBe("DreamShaper, version 7");
    const file = screen.getByRole("combobox", { name: "File" }) as HTMLSelectElement;
    await userEvent.selectOptions(file, file.options[1]!);
    await waitFor(() => app.dl.card.plan?.fileName === "dreamshaper_7-full.safetensors");
  });

  it("links only in the installs ticked, and keeps that choice for next time", async () => {
    const { app, engine } = await mount();
    await read(DREAM);
    await userEvent.click(screen.getByRole("checkbox", { name: "Link it in ComfyUI-Sandbox" }));
    expect(text()).toContain("Then it will be linked in 1 install.");
    await userEvent.click(button("Download 2.0 GB"));
    await waitFor(() => app.dl.downloads().length === 1);
    expect(app.dl.downloads()[0]!.installIds).toEqual(["studio"]);
    void engine;
    // The card is done with, and the field is empty for the next address.
    expect(card()).toBeNull();
    expect(field().value).toBe("");

    await read("https://civitai.com/models/58390");
    expect(app.dl.card.ticked).toEqual(["studio"]);
  });

  it("says a file with no install ticked stays only in the vault", async () => {
    await mount();
    await read(DREAM);
    await userEvent.click(screen.getByRole("checkbox", { name: "Link it in ComfyUI-Studio" }));
    await userEvent.click(screen.getByRole("checkbox", { name: "Link it in ComfyUI-Sandbox" }));
    expect(text()).toContain(
      "No install is ticked. The file will only be in the vault, and you can link it later from Library.",
    );
    expect(button("Download 2.0 GB")).toBeEnabled();
  });
});

describe("the plan for a Hugging Face file", () => {
  it("cannot download until a folder is chosen", async () => {
    const { app } = await mount();
    await read(FLUX_FP8);
    expect(card()!.querySelector(".card-sub")!.textContent).toBe("Comfy-Org/flux1-dev");
    expect(button("Choose a folder first")).toBeDisabled();
    expect(text()).toContain("the folder you choose, as flux1-dev-fp8.safetensors");
    await userEvent.selectOptions(screen.getByRole("combobox", { name: "Folder" }), "diffusion_models");
    await waitFor(() => app.dl.card.plan?.category === "diffusion_models");
    expect(button(/^Download 16 GB$/)).toBeEnabled();
    // A folder the person chose carries no reason.
    expect(text()).not.toContain("(Civitai");
  });

  it("holds an install whose folder has a different file with that name", async () => {
    const { app } = await mount((e) =>
      e.downloads.devPlaceFile("sandbox", "diffusion_models", "flux1-dev-fp8.safetensors"),
    );
    await read(FLUX_FP8);
    await userEvent.selectOptions(screen.getByRole("combobox", { name: "Folder" }), "diffusion_models");
    await waitFor(() => app.dl.card.plan?.category === "diffusion_models");
    expect(text()).toContain("a different file already has this name in its diffusion_models folder");
    expect(screen.queryByRole("checkbox", { name: "Link it in ComfyUI-Sandbox" })).toBeNull();
    expect(app.dl.card.ticked).toEqual(["studio"]);
  });

  it("will not download when the vault drive lacks the room, and says both numbers", async () => {
    await mount((e) => e.downloads.devSetFreeBytes(12 * 1024 ** 3));
    await read(FLUX_FP8);
    await userEvent.selectOptions(screen.getByRole("combobox", { name: "Folder" }), "diffusion_models");
    await waitFor(() => text().includes("Not enough space"));
    expect(text()).toContain(
      "Drive C: has 12 GB free, and this file needs 16 GB. Free some space, then read the address again.",
    );
    expect(button("Not enough space on drive C:")).toBeDisabled();
  });
});

describe("a file the vault already holds", () => {
  it("downloads nothing and offers only the links that are missing", async () => {
    const { app } = await mount(async (e) => {
      e.devSetSymlinksSupported(true);
      e.devSetComfyRunning(false);
      const scan = (await e.getLastScan())!;
      const plan = await e.buildPlan(scan.scanId);
      await e.startApply({ planId: plan.planId, groupIds: plan.groups.map((g) => g.groupId) });
      e.devFinish();
    });
    await read(T5);
    const ok = card()!.querySelector(".verdict.ok")!;
    expect(ok.querySelector("h4")!.textContent).toBe("Already in the vault");
    expect(ok.querySelector("p")!.textContent!.replace(/\s+/g, " ")).toBe(
      "The vault already has this file as text_encoders\\t5xxl_fp16.safetensors, with the same SHA-256. Nothing will be downloaded.",
    );
    expect(text()).toContain("already has this link");
    expect(button("Every install already has this link")).toBeDisabled();
    expect(text()).not.toContain("Will download");
    void app;
  });
});

describe("adding only the links a held file is missing", () => {
  it("links it in the install that lacks it, and downloads nothing", async () => {
    let sha = "";
    const { app, engine } = await mount(async (e) => {
      e.devSetSymlinksSupported(true);
      e.devSetComfyRunning(false);
      const scan = (await e.getLastScan())!;
      const plan = await e.buildPlan(scan.scanId);
      await e.startApply({ planId: plan.planId, groupIds: plan.groups.map((g) => g.groupId) });
      e.devFinish();
      const t5 = (await e.listVaultFiles({ offset: 0, limit: 1000 })).files.find(
        (f) => f.canonicalName === "t5xxl_fp16.safetensors",
      )!;
      sha = t5.sha256;
      const sandbox = t5.links.find((l) => l.installId === "sandbox")!;
      await e.removeLink(sandbox.id);
    });
    await read(T5);
    await userEvent.click(button("Add 1 link"));
    await waitFor(() => app.dl.downloads().length === 1);
    const row = app.dl.downloads()[0]!;
    expect(row.state).toBe("linkedOnly");
    expect(row.bytesDone).toBe(0);
    expect(row.linkedInstallIds).toEqual(["sandbox"]);
    const links = await engine.listLinks({ sha256: sha });
    expect(links.some((l) => l.installId === "sandbox")).toBe(true);
    expect(document.querySelector(".dlrow .dlrow-s")!.textContent).toBe(
      "Linked in ComfyUI-Sandbox. It was already in the vault, so nothing was downloaded.",
    );
  });
});

describe("a token the service stopped accepting", () => {
  it("says so on the card, in the service's words", async () => {
    await mount(async (e) => {
      await e.setToken("huggingface", "hf_good");
      e.downloads.devRevokeToken("huggingface");
    });
    await read(GATED);
    const verdict = card()!.querySelector(".verdict.no")!;
    expect(verdict.querySelector("h4")!.textContent).toBe("Hugging Face did not accept your token");
    expect(verdict.querySelector("q")!.textContent).toBe("Invalid username or password.");
    expect(verdict.textContent).toContain("Paste a new token in Settings, then read the address again.");
  });
});

describe("a refusal found while reading", () => {
  it("says what is wrong, what Hugging Face said word for word, and what to do", async () => {
    const { app } = await mount();
    await read(GATED);
    const verdict = card()!.querySelector(".verdict.no")!;
    expect(verdict.querySelector("h4")!.textContent).toBe("Hugging Face needs your token for this model");
    expect(verdict.querySelector("q")!.textContent).toBe(
      "Access to model black-forest-labs/FLUX.1-dev is restricted. You must have access to it and be authenticated to access it. Please log in.",
    );
    expect(verdict.textContent).toContain(
      "Add your Hugging Face token in Settings, then read the address again.",
    );
    await userEvent.click(button("Open Settings"));
    expect(app.screen()).toBe("settings");
  });

  it("opens the model's page when the terms are not accepted yet", async () => {
    const { engine } = await mount((e) => void e.setToken("huggingface", "hf_good"));
    await read(GATED);
    expect(card()!.querySelector("h4")!.textContent).toBe(
      "Your Hugging Face account has no access to this model yet",
    );
    await userEvent.click(button("Open the model's page"));
    await waitFor(() => engine.opened.length > 0);
    expect(engine.opened.at(-1)).toBe("https://huggingface.co/black-forest-labs/FLUX.1-dev");
  });

  it("says Civitai needs a token, in Civitai's words", async () => {
    await mount();
    await read("https://civitai.com/models/123456");
    expect(card()!.querySelector("h4")!.textContent).toBe("Civitai needs your token for this model");
    expect(card()!.querySelector("q")!.textContent).toBe(
      "The creator of this asset requires you to be logged in to download it",
    );
  });

  it("reads the address again from the refusal", async () => {
    const { app, engine } = await mount();
    await read(GATED);
    await engine.setToken("huggingface", "hf_good");
    engine.downloads.devAcceptTerms("black-forest-labs", "FLUX.1-dev");
    await userEvent.click(button("Read the address again"));
    await waitFor(() => app.dl.card.plan !== null && app.dl.card.refusal === null);
    expect(card()!.querySelector(".verdict.no")).toBeNull();
  });
});

describe("the list of downloads", () => {
  async function started() {
    const h = await mount();
    await read(DREAM);
    await userEvent.click(button("Download 2.0 GB"));
    await waitFor(() => h.app.dl.downloads().length === 1);
    return h;
  }
  const row = () => document.querySelector(".dlrow")!;

  it("says nothing has been downloaded yet", async () => {
    await mount();
    expect(text()).toContain(
      "Nothing has been downloaded yet. A download you start shows here, and it stays here until ComfyVault closes.",
    );
  });

  it("follows a running download, and Stop keeps the part", async () => {
    const { app, engine } = await started();
    for (let i = 0; i < 20; i++) engine.downloads.devStep();
    await waitFor(() => row().textContent!.includes("Downloading:"));
    expect(row().querySelector(".dlrow-s")!.textContent).toMatch(
      /^Downloading: \S+ (MB|GB) of 2\.0 GB, \d+(\.\d)? MB\/s, (less than a minute|about \d+ minutes?) left\.$/,
    );
    expect(document.querySelector(".sec .n")!.textContent).toBe("1 is not finished");
    await userEvent.click(button("Stop"));
    await waitFor(() => app.dl.downloads()[0]!.state === "stopped");
    expect(row().textContent).toContain("The part already downloaded is kept, so it can continue from there.");
    await userEvent.click(button("Continue"));
    await waitFor(() => app.dl.downloads()[0]!.state === "running");
  });

  it("asks before discarding a kept part, and says what goes", async () => {
    const { app, engine } = await started();
    for (let i = 0; i < 20; i++) engine.downloads.devStep();
    await engine.stopDownload(app.dl.downloads()[0]!.downloadId);
    await waitFor(() => app.dl.downloads()[0]!.state === "stopped");
    const done = app.dl.downloads()[0]!.bytesDone;
    await userEvent.click(button("Discard it"));
    await waitFor(() => document.querySelector(".modal") !== null);
    expect(document.querySelector(".modal")!.textContent).toContain(
      `The ${fmt(done)} already downloaded is deleted. The model is not in the vault and not linked anywhere.`,
    );
    const confirm = [...document.querySelectorAll(".modal button")].find(
      (b) => b.textContent === "Discard it",
    ) as HTMLButtonElement;
    await userEvent.click(confirm);
    await waitFor(() => app.dl.downloads().length === 0);
  });

  it("says what happened when it is done, and opens the model in Library", async () => {
    const { app, engine } = await started();
    engine.downloads.devFinishDownloads();
    await waitFor(() => app.dl.downloads()[0]!.state === "done");
    expect(row().querySelector(".dlrow-s")!.textContent).toBe(
      "Downloaded into the vault as checkpoints\\dreamshaper_8.safetensors, and linked in ComfyUI-Studio and ComfyUI-Sandbox.",
    );
    expect(document.querySelector(".sec .n")!.textContent).toBe("all finished");
    await userEvent.click(button("Show it in Library"));
    await waitFor(() => app.screen() === "library");
    expect(app.lib.selected).toBe("879DB523C30D3B9017143D56705015E15A2CB5628762C11D086FED9538ABD7FD");
  });

  it("says a file that did not match was deleted, and can download it again", async () => {
    const h = await mount((e) => e.downloads.devCorruptNext());
    await read(DREAM);
    await userEvent.click(button("Download 2.0 GB"));
    await waitFor(() => h.app.dl.downloads().length === 1);
    h.engine.downloads.devFinishDownloads();
    await waitFor(() => h.app.dl.downloads()[0]!.state === "mismatch");
    expect(row().textContent).toContain(
      "The downloaded file did not match the SHA-256 Civitai gave, so it was deleted.",
    );
    await userEvent.click(button("Download it again"));
    await waitFor(() => h.app.dl.downloads()[0]!.state === "running");
    expect(h.app.dl.downloads()[0]!.bytesDone).toBe(0);
  });

  it("says a dropped connection kept the part", async () => {
    const { app, engine } = await started();
    engine.downloads.devStep();
    engine.downloads.devDropConnection();
    await waitFor(() => app.dl.downloads()[0]!.state === "failed");
    expect(row().querySelector(".bad")!.textContent).toMatch(/^The connection to Civitai dropped at /);
  });
});

describe("a download ComfyVault closed on", () => {
  it("shows a banner on Download, a line on Home, and continues from the kept part", async () => {
    const engine = new FixtureEngine({ manual: true });
    const r = await engine.startDownload({ address: DREAM, category: "checkpoints", installIds: [] });
    for (let i = 0; i < 10; i++) engine.downloads.devStep();
    engine.downloads.devCutOff();
    harness = await renderWithApp(() => <App />, { engine });
    const { app } = harness;
    await waitFor(() => app.dl.downloads().length === 1);
    const kept = app.dl.downloads()[0]!.bytesDone;

    await waitFor(() => text().includes("was cut off at"));
    expect(text()).toContain(
      `dreamshaper_8.safetensors was cut off at ${fmt(kept)} of 2.0 GB when ComfyVault closed.`,
    );
    await userEvent.click(button("Continue it or discard it"));
    expect(app.screen()).toBe("download");
    const banner = document.querySelector(".banner")!;
    expect(banner.textContent).toBe(
      `A download was cut offComfyVault closed while dreamshaper_8.safetensors was downloading. ${fmt(kept)} of 2.0 GB is kept. Continue it from there, or discard it.`,
    );
    await userEvent.click(button("Continue"));
    await waitFor(() => app.dl.downloads()[0]!.state === "running");
    expect(app.dl.downloads()[0]!.bytesDone).toBe(kept);
    expect(document.querySelector(".banner")).toBeNull();
    void r;
  });

  it("counts running and waiting downloads on the rail", async () => {
    const engine = new FixtureEngine({ manual: true });
    await engine.startDownload({ address: DREAM, category: "checkpoints", installIds: [] });
    await engine.startDownload({ address: "https://civitai.com/models/58390", category: "loras", installIds: [] });
    harness = await renderWithApp(() => <App />, { engine });
    await waitFor(() => harness!.app.dl.downloads().length === 2);
    const nav = screen.getAllByRole("button").find((b) => /^Download/.test(b.textContent ?? ""))!;
    expect(nav.textContent).toBe("Download2");
  });
});
