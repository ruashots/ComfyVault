import { screen } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";

import { App } from "~/App";
import { FixtureEngine } from "~/ipc/fixture/engine";
import type { VaultFile } from "~/ipc/contract";
import { renderWithApp, waitFor, type Harness } from "~/test/render";

let harness: Harness | null = null;
afterEach(() => {
  harness?.unmount();
  harness = null;
});

const dialog = () => document.querySelector('[role="dialog"]');
const text = (el: Element | null | undefined) => (el?.textContent ?? "").replace(/\s+/g, " ").trim();
const heading = () => text(dialog()?.querySelector("h2"));
const nodes = () => [...dialog()!.querySelectorAll(".tnode")] as HTMLButtonElement[];
const button = (name: string) =>
  [...dialog()!.querySelectorAll("button")].find((b) => text(b) === name) as HTMLButtonElement | undefined;

/** A vault model nothing links to yet, with its details open. */
async function details(prepare?: (engine: FixtureEngine, model: VaultFile) => void) {
  const engine = new FixtureEngine();
  const model = (await engine.listOrphans())[0]!;
  prepare?.(engine, model);
  harness = await renderWithApp(() => <App />, { engine });
  const { app } = harness;
  app.actions.go("library");
  await waitFor(() => app.library().some((r) => r.sha256 === model.sha256));
  app.setLib({ selected: model.sha256, drawerOpen: true });
  await waitFor(() => screen.queryAllByRole("button", { name: /Link into an install/ }).length > 0);
  return { ...harness, model };
}

async function pickStudio() {
  await userEvent.click(screen.getByRole("button", { name: /Link into an install/ }));
  await waitFor(() => heading() === "Link into an install");
  await userEvent.click(nodes().find((n) => text(n).startsWith("ComfyUI-Studio"))!);
  await waitFor(() => heading() === "ComfyUI-Studio · Choose a folder" && nodes().length > 0);
}

describe("linking a model into an install from the Library", () => {
  it("first asks which install, and says which already have it", async () => {
    const { engine, model } = await details();
    await engine.createLink({ installId: "sandbox", sha256: model.sha256, relativeDir: `models\\${model.category}` });
    await harness!.app.actions.refresh();
    await userEvent.click(screen.getByRole("button", { name: /Link into an install/ }));
    await waitFor(() => heading() === "Link into an install");
    const rows = nodes().map((n) => [text(n.querySelector("span")), text(n.querySelector(".hint")), n.disabled]);
    expect(rows).toEqual([
      ["ComfyUI-Studio", "", false],
      ["ComfyUI-Sandbox", "Already linked", true],
    ]);
    // Nothing to link until a folder is picked.
    expect(button("Link here")).toBeUndefined();
    expect(button("Cancel")).toBeDefined();
  });

  it("then offers the folder used last time first, picked, and says where the link will be", async () => {
    const { model } = await details((engine, m) =>
      engine.downloads.remember("studio", m.category, `C:\\ComfyUI-Studio\\models\\${m.category}\\mine`),
    );
    await pickStudio();
    const first = nodes()[0]!;
    expect(text(first.querySelector("span"))).toBe(`models\\${model.category}\\mine`);
    expect(text(first.querySelector(".hint"))).toBe("Last used");
    expect(first.classList.contains("on")).toBe(true);
    expect(nodes().filter((n) => text(n).includes("Last used"))).toHaveLength(1);
    expect(text(dialog()!.querySelector(".res"))).toBe(
      `C:\\ComfyUI-Studio\\models\\${model.category}\\mine\\${model.canonicalName}`,
    );
    expect(button("Link here")!.disabled).toBe(false);
  });

  it("with no folder used before, picks where ComfyUI keeps this kind, and marks none as last used", async () => {
    const { model } = await details();
    await pickStudio();
    const picked = nodes().find((n) => n.classList.contains("on"))!;
    expect(text(picked)).toBe(`models\\${model.category}`);
    expect(text(dialog())).not.toContain("Last used");
  });

  it("makes a new folder inside the picked one, picks it, and refuses a name Windows would not take", async () => {
    const { model } = await details();
    await pickStudio();
    await userEvent.click(button("New folder")!);
    const field = screen.getByRole("textbox", { name: "Name of the new folder" });
    await userEvent.type(field, "a/b");
    await userEvent.click(button("Create")!);
    expect(text(dialog()!.querySelector(".tnew-err"))).toContain("A folder name cannot contain");

    await userEvent.clear(field);
    await userEvent.type(field, "my-upscalers");
    await userEvent.click(button("Create")!);
    const labels = nodes().map((n) => text(n));
    const at = labels.indexOf(`models\\${model.category}`);
    expect(labels[at + 1]).toBe(`models\\${model.category}\\my-upscalers`);
    expect(nodes()[at + 1]!.classList.contains("on")).toBe(true);
    expect(text(dialog()!.querySelector(".res"))).toBe(
      `C:\\ComfyUI-Studio\\models\\${model.category}\\my-upscalers\\${model.canonicalName}`,
    );
  });

  it("stays open with the engine's words when a different file already has the name there", async () => {
    const { engine, model } = await details();
    engine.devTakePath(`C:\\ComfyUI-Studio\\models\\${model.category}\\${model.canonicalName}`);
    await pickStudio();
    await userEvent.click(button("Link here")!);
    await waitFor(() => dialog()?.querySelector(".verdict.no") != null);
    expect(text(dialog()!.querySelector(".verdict.no"))).toBe(
      "A different file with this name is already here. Choose another folder.",
    );
    expect(await engine.listLinks({ sha256: model.sha256 })).toEqual([]);
  });

  it("links it there, says so, and lists the new link in the details", async () => {
    const { app, engine, model } = await details();
    await pickStudio();
    await userEvent.click(button("Link here")!);
    await waitFor(() => dialog() === null);
    expect(app.toast()!.message).toBe("Linked in ComfyUI-Studio.");
    const links = await engine.listLinks({ sha256: model.sha256 });
    expect(links.map((l) => l.absPath)).toEqual([
      `C:\\ComfyUI-Studio\\models\\${model.category}\\${model.canonicalName}`,
    ]);
    await waitFor(() => document.querySelectorAll(".reach .r").length === 1);
    expect(text(document.querySelector(".reach .r .top"))).toContain("ComfyUI-Studio");
  });
});
