import { render, screen } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";

import { App } from "~/App";
import { FixtureEngine } from "~/ipc/fixture/engine";
import type { ContentRow, ModelMetadata } from "~/ipc/contract";
import { CivitaiPicture } from "~/screens/Library";
import { renderWithApp, waitFor, type Harness } from "~/test/render";

let harness: Harness | null = null;

afterEach(() => {
  harness?.unmount();
  harness = null;
});

const text = () => (document.body.textContent ?? "").replace(/\s+/g, " ");

async function open(prepare?: (engine: FixtureEngine) => void | Promise<void>): Promise<Harness> {
  const engine = new FixtureEngine({ manual: true });
  await prepare?.(engine);
  const h = await renderWithApp(() => <App />, { engine });
  harness = h;
  return h;
}

/** Open the first Library row that passes `pick` in the drawer. */
async function openRow(h: Harness, pick: (row: ContentRow) => boolean) {
  h.app.actions.go("library");
  await waitFor(() => h.app.library().length > 0);
  const row = h.app.library().find(pick)!;
  h.app.setLib({ selected: row.sha256, drawerOpen: true });
  await waitFor(() => text().includes("Civitai"));
  return row;
}

describe("the Civitai lookup", () => {
  it("runs in the background with the switch on, and the Library shows what came back", async () => {
    const h = await open();
    await waitFor(() => h.app.lookup().kind === "done");
    expect(h.engine.devCivitaiRequests()).toBeGreaterThan(0);

    // Nothing was cached before the lookup: the double starts empty, as the
    // engine does, so every answer on screen came from this pass.
    const found = await openRow(h, (r) => r.metadata?.found === true);
    const meta = found.metadata!;
    await waitFor(() => text().includes(meta.modelName!));
    expect(screen.getByRole("button", { name: /Open on Civitai/ })).toBeTruthy();
  });

  it("says a file Civitai does not know is normal, not an error", async () => {
    const h = await open();
    await waitFor(() => h.app.lookup().kind === "done");
    await openRow(h, (r) => r.metadata?.found === false);
    expect(text()).toContain("Civitai has no file with this fingerprint. That is normal");
  });

  it("asks each file once: a second pass sends nothing for files already answered", async () => {
    const h = await open();
    await waitFor(() => h.app.lookup().kind === "done");
    const sent = h.engine.devCivitaiRequests();
    await h.engine.startScan();
    h.engine.devFinish();
    await waitFor(() => h.app.lookup().kind === "done");
    await h.app.actions.refresh();
    expect(h.engine.devCivitaiRequests()).toBe(sent);
  });

  it("sends nothing with the switch off, and says why nothing is shown", async () => {
    const h = await open(async (e) => {
      await e.updateSettings({ metadataLookupsEnabled: false });
    });
    await h.app.actions.refresh();
    expect(h.engine.devCivitaiRequests()).toBe(0);
    await openRow(h, () => true);
    expect(text()).toContain("Civitai lookup is off, so nothing was asked about this file.");
    expect(h.engine.devCivitaiRequests()).toBe(0);
  });

  it("asks once the switch is turned on", async () => {
    const h = await open(async (e) => {
      await e.updateSettings({ metadataLookupsEnabled: false });
    });
    expect(h.engine.devCivitaiRequests()).toBe(0);
    h.app.actions.go("settings");
    await waitFor(() => text().includes("Civitai lookup"));
    await userEvent.click(screen.getByRole("switch", { name: /Ask Civitai what each file is/ }));
    await waitFor(() => h.app.lookup().kind === "done");
    expect(h.engine.devCivitaiRequests()).toBeGreaterThan(0);
  });

  it("offline, says Civitai could not be reached and does not ask again until the next scan", async () => {
    const h = await open((e) => e.devSetCivitaiReachable(false));
    await waitFor(() => h.app.lookup().kind === "unreachable");
    await openRow(h, () => true);
    expect(text()).toContain("Could not reach Civitai");
    expect(text()).toContain("ComfyVault asks again after the next scan.");
    expect(text()).not.toContain("Civitai has no file with this fingerprint");
    h.app.actions.go("settings");
    await waitFor(() => text().includes("Civitai lookup"));
    expect(text()).toContain("Could not reach Civitai");
    h.app.actions.go("library");

    // Other work refreshes the screen often. None of it may retry the lookup.
    h.engine.devSetCivitaiReachable(true);
    await h.app.actions.refresh();
    await h.app.actions.refresh();
    expect(h.app.lookup().kind).toBe("unreachable");
    expect(h.engine.devCivitaiRequests()).toBe(0);

    await h.engine.startScan();
    h.engine.devFinish();
    await waitFor(() => h.app.lookup().kind === "done");
    expect(h.engine.devCivitaiRequests()).toBeGreaterThan(0);
  });
});

const meta = (over: Partial<ModelMetadata>): ModelMetadata => ({
  sha256: "A".repeat(64),
  source: "civitai",
  fetchedAt: "2026-09-25T00:00:00Z",
  found: true,
  modelName: "Some model",
  modelType: "Checkpoint",
  versionName: "1",
  baseModel: "SDXL 1.0",
  triggerWords: [],
  nsfw: false,
  nsfwLevel: 1,
  civitaiModelId: 1,
  civitaiVersionId: 1,
  pageUrl: "https://civitai.com/models/1",
  downloadUrl: null,
  previewImageUrls: ["https://image.civitai.com/x/1.jpeg"],
  previewImages: [],
  ambiguous: false,
  ...over,
});

const pic = (nsfwLevel: number, n: number, type = "image") => ({
  url: `https://image.civitai.com/x/${n}.jpeg`,
  nsfwLevel,
  type,
});

describe("the Civitai picture", () => {
  it("shows the first picture when it is rated PG or PG-13", () => {
    const { unmount } = render(() => (
      <CivitaiPicture meta={meta({ previewImages: [pic(2, 1), pic(1, 2), pic(8, 3)] })} />
    ));
    expect(screen.getByRole("img").getAttribute("src")).toBe("https://image.civitai.com/x/1.jpeg");
    unmount();
  });

  it("skips adult, unrated and video entries to the first safe picture", () => {
    const { unmount } = render(() => (
      <CivitaiPicture
        meta={meta({
          nsfwLevel: 11,
          previewImages: [pic(8, 1), pic(0, 2), pic(1, 3, "video"), pic(16, 4), pic(1, 5), pic(2, 6)],
        })}
      />
    ));
    expect(screen.getByRole("img").getAttribute("src")).toBe("https://image.civitai.com/x/5.jpeg");
    unmount();
  });

  it("shows none, and says so in one line, when no picture is rated PG or PG-13", () => {
    const { unmount } = render(() => (
      <CivitaiPicture meta={meta({ previewImages: [pic(4, 1), pic(8, 2), pic(0, 3)] })} />
    ));
    expect(screen.queryByRole("img")).toBeNull();
    expect(document.body.textContent).toContain("Civitai has no picture of this model rated PG or PG-13");
    unmount();
  });

  it("says nothing when Civitai has no picture at all", () => {
    const { container, unmount } = render(() => <CivitaiPicture meta={meta({ previewImages: [] })} />);
    expect(container.textContent).toBe("");
    unmount();
  });
});
