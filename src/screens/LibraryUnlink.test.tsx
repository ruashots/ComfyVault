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

/** Everything consolidated, then the Library with one model's details open. */
async function drawerFor(pick: (rows: readonly ContentRow[]) => ContentRow | undefined) {
  const engine = new FixtureEngine({ manual: true });
  engine.devSetSymlinksSupported(true);
  engine.devSetComfyRunning(false);
  const scan = (await engine.getLastScan())!;
  const plan = await engine.buildPlan(scan.scanId);
  await engine.startApply({ planId: plan.planId, groupIds: plan.groups.map((g) => g.groupId) });
  engine.devFinish();
  harness = await renderWithApp(() => <App />, { engine });
  const row = pick(harness.app.library())!;
  harness.app.setLib({ selected: row.sha256, drawerOpen: true });
  harness.app.actions.go("library");
  await waitFor(() => reach().length === row.occurrenceCount);
  return { ...harness, row };
}

const reach = () => [...document.querySelectorAll(".reach .r")];
const rowFor = (install: string) =>
  reach().find((r) => r.querySelector(".top")!.textContent!.includes(install))!;
const text = (el: Element | null) => (el?.textContent ?? "").replace(/\s+/g, " ").trim();
const modal = () => document.querySelector('[role="dialog"]');
const lines = () => [...modal()!.querySelectorAll(".mb .note")].map((n) => text(n));

async function unlink(install: string) {
  await userEvent.click(rowFor(install).querySelector("button")!);
  await waitFor(() => modal() !== null);
}

/** A model linked in both installs, and named by saved workflows. */
const inBoth = (rows: readonly ContentRow[]) =>
  rows.find((r) => r.inVault && r.installIds.length === 2 && r.occurrenceCount === 2);

describe("the details of a model linked into installs", () => {
  it("offer to unlink it from each install, and to link it into another under the list", async () => {
    const { row } = await drawerFor(inBoth);
    expect(reach()).toHaveLength(2);
    for (const r of reach()) expect(text(r.querySelector(".top button"))).toBe("Unlink");
    expect(document.querySelector(".reach .pill")).toBeNull();
    // The link button comes after the installs.
    const link = screen.getByRole("button", { name: /Link into an install/ });
    expect(document.querySelector(".reach")!.compareDocumentPosition(link) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(row.installIds).toEqual(expect.arrayContaining(["studio", "sandbox"]));
  });
});

describe("unlinking a model from an install", () => {
  it("names the saved workflows there that will miss it, and says the rest stays", async () => {
    const { engine, row } = await drawerFor(inBoth);
    const [usage] = await engine.checkModelUsage([row.name]);
    const inStudio = usage!.matches.filter((m) => m.installId === "studio").map((m) => m.workflowName);
    await unlink("ComfyUI-Studio");
    expect(text(modal()!.querySelector("h2"))).toBe("Unlink from ComfyUI-Studio?");
    expect(lines()).toEqual(
      inStudio.length > 0
        ? [
            `Missing from ${inStudio.length} saved ${inStudio.length === 1 ? "workflow" : "workflows"}:`,
            inStudio.join(" · "),
            "Vault copy and other links stay.",
          ]
        : ["Vault copy and other links stay."],
    );
    expect(inStudio.length).toBeGreaterThan(0);
  });

  it("names no workflows for an install none of its saved workflows use it in", async () => {
    await drawerFor(inBoth);
    await unlink("ComfyUI-Sandbox");
    expect(lines()).toEqual(["Vault copy and other links stay."]);
  });

  it("says the vault copy stays and where to find it when it is the last link", async () => {
    await drawerFor((rows) => rows.find((r) => r.inVault && r.occurrenceCount === 1));
    const install = text(reach()[0]!.querySelector(".top span"));
    await unlink(install);
    expect(lines()).toContain("Vault copy stays. Find it in Cleanup.");
  });

  it("asks to close that install's ComfyUI when Windows will not let the link go", async () => {
    const { engine, row } = await drawerFor(inBoth);
    const studio = (await engine.listLinks({ sha256: row.sha256 })).find((l) => l.installId === "studio")!;
    engine.devLockLink(studio.id);
    await unlink("ComfyUI-Studio");
    await userEvent.click(dialogButton("Unlink"));
    await waitFor(() => lines()[0] === "Close ComfyUI-Studio to unlink this model.");
    expect(lines()).toEqual(["Close ComfyUI-Studio to unlink this model."]);
    expect(dialogButton("Unlink").disabled).toBe(true);
    expect(modal()!.querySelector(".verdict")).toBeNull();
    expect((await engine.listLinks({ sha256: row.sha256 })).length).toBe(2);
  });

  it("unlinks while that install's ComfyUI runs, as Windows allows", async () => {
    const { engine, row } = await drawerFor(inBoth);
    engine.devSetRunningInstalls(["studio"]);
    await unlink("ComfyUI-Studio");
    expect(lines()).not.toContain("Close ComfyUI-Studio to unlink this model.");
    await userEvent.click(dialogButton("Unlink"));
    await waitFor(() => modal() === null);
    expect((await engine.listLinks({ sha256: row.sha256 })).map((l) => l.installId)).toEqual(["sandbox"]);
  });

  it("removes that link, says so, and shows the model's other links", async () => {
    const { app, engine, row } = await drawerFor(inBoth);
    await unlink("ComfyUI-Studio");
    await userEvent.click(dialogButton("Unlink"));
    await waitFor(() => modal() === null || modal()!.querySelector(".verdict") !== null);
    expect(text(modal()?.querySelector(".verdict") ?? null)).toBe("");
    await waitFor(() => reach().length === 1);
    expect(text(reach()[0]!.querySelector(".top span"))).toBe("ComfyUI-Sandbox");
    expect(text(document.querySelector(".reach")!.previousElementSibling!.querySelector(".n"))).toBe("1");
    expect(app.toast()!.message).toBe("Unlinked from ComfyUI-Studio.");
    const links = await engine.listLinks({ sha256: row.sha256 });
    expect(links.map((l) => l.installId)).toEqual(["sandbox"]);
  });
});

/** A button in the dialog, not in the drawer behind it. */
function dialogButton(name: string): HTMLButtonElement {
  return [...modal()!.querySelectorAll("button")].find((b) => text(b) === name) as HTMLButtonElement;
}
