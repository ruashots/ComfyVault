import { screen } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";

import { App } from "~/App";
import { FixtureEngine } from "~/ipc/fixture/engine";
import type { ContentRow } from "~/ipc/contract";
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

  it("loads no picture for a model Civitai knows", async () => {
    const h = await open();
    await waitFor(() => h.app.lookup().kind === "done");
    const found = await openRow(h, (r) => r.metadata?.found === true);
    await waitFor(() => text().includes(found.metadata!.modelName!));
    // Nothing in the window loads a remote image.
    const remote = [...document.querySelectorAll("img")].filter((img) =>
      /^https?:/i.test(img.getAttribute("src") ?? ""),
    );
    expect(remote).toEqual([]);
    expect(document.querySelectorAll("img").length).toBe(0);
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
