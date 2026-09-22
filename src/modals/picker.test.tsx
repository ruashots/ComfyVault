import { screen } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";

import { FixtureEngine } from "~/ipc/fixture/engine";
import {
  PickerModalView,
  openInstancePicker,
  openLinkPicker,
  openVaultPicker,
} from "~/modals/picker";
import { renderWithApp, waitFor, type Harness } from "~/test/render";

let harness: Harness | null = null;

afterEach(() => {
  harness?.unmount();
  harness = null;
});

async function mountPicker(
  open: (h: Harness) => Promise<void>,
): Promise<Harness> {
  const engine = new FixtureEngine();
  harness = await renderWithApp(() => <PickerModalView />, { engine });
  await open(harness);
  await waitFor(() => screen.queryAllByRole("dialog").length > 0);
  return harness;
}

const node = (name: string) =>
  screen.getAllByRole("button").find((b) => b.textContent?.startsWith(name))!;

const expander = (name: string) =>
  screen.getByRole("button", { name: `Open ${name}` });

const verdict = () => document.querySelector(".verdict");
const confirmButton = (label: string) => screen.getByRole("button", { name: label });

describe("the person never types a path", () => {
  it("has no text field until New folder is used", async () => {
    await mountPicker((h) => openInstancePicker(h.app));
    expect(document.querySelectorAll("input")).toHaveLength(0);
  });

  it("starts at the drives and walks down from there", async () => {
    await mountPicker((h) => openInstancePicker(h.app));
    expect(node("C:\\")).toBeInTheDocument();
    expect(node("D:\\")).toBeInTheDocument();

    await userEvent.click(expander("C:\\"));
    await waitFor(() => screen.queryAllByRole("button", { name: /Open Users/ }).length > 0);
    expect(node("ComfyUI-Portable")).toBeInTheDocument();

    await userEvent.click(expander("Users"));
    await waitFor(() => screen.queryAllByRole("button", { name: /Open alex/ }).length > 0);
    await userEvent.click(expander("alex"));
    await waitFor(() => screen.queryAllByRole("button", { name: /Downloads/ }).length > 0);
    expect(node("Downloads")).toBeInTheDocument();
  });

  it("closes a folder again and takes its children with it", async () => {
    await mountPicker((h) => openInstancePicker(h.app));
    await userEvent.click(expander("C:\\"));
    await waitFor(() => screen.queryAllByRole("button", { name: /Open Users/ }).length > 0);
    await userEvent.click(screen.getByRole("button", { name: "Collapse C:\\" }));
    await waitFor(() => screen.queryAllByRole("button", { name: /Open Users/ }).length === 0);
    expect(screen.queryByRole("button", { name: /Open Users/ })).toBeNull();
  });
});

describe("registering an install", () => {
  it("refuses a folder with no models folder inside, and says what to pick", async () => {
    const h = await mountPicker((x) => openInstancePicker(x.app));
    await userEvent.click(expander("C:\\"));
    await waitFor(() => screen.queryAllByRole("button", { name: /Open Users/ }).length > 0);
    await userEvent.click(node("Users"));
    await waitFor(() => verdict() !== null && !h.app.modal()!.kind.endsWith("x"));
    await waitFor(() => verdict()?.classList.contains("no") === true);

    expect(verdict()!.textContent).toContain("Not a ComfyUI install");
    expect(verdict()!.textContent).toContain("the one with main.py in it");
    expect(confirmButton("Add this install")).toBeDisabled();
  });

  it("refuses an install that is registered already", async () => {
    await mountPicker((h) => openInstancePicker(h.app));
    await userEvent.click(expander("C:\\"));
    await waitFor(() => screen.queryAllByRole("button", { name: /Open Users/ }).length > 0);
    await userEvent.click(node("ComfyUI-Alpha"));
    await waitFor(() => verdict()?.classList.contains("no") === true);

    expect(verdict()!.textContent).toContain("Already registered");
    expect(confirmButton("Add this install")).toBeDisabled();
  });

  it("refuses a folder Windows will not open", async () => {
    await mountPicker((h) => openInstancePicker(h.app));
    await userEvent.click(expander("C:\\"));
    await waitFor(() => screen.queryAllByRole("button", { name: /Open Users/ }).length > 0);
    const locked = node("Program Files");
    expect(locked).toBeDisabled();
    expect(locked.textContent).toContain("cannot be opened");
  });

  it("accepts a real install and reports what is inside", async () => {
    await mountPicker((h) => openInstancePicker(h.app));
    await userEvent.click(expander("C:\\"));
    await waitFor(() => screen.queryAllByRole("button", { name: /Open Users/ }).length > 0);
    await userEvent.click(node("ComfyUI-Portable"));
    await waitFor(() => verdict()?.classList.contains("ok") === true);

    expect(verdict()!.textContent).toContain("This is a ComfyUI install");
    expect(verdict()!.textContent).toContain("6 folders");
    expect(verdict()!.textContent).toContain("31 files");
    expect(verdict()!.textContent).toContain("115 GB");
    expect(confirmButton("Add this install")).toBeEnabled();
  });

  it("warns that an install on another drive is copied, not moved", async () => {
    await mountPicker((h) => openInstancePicker(h.app));
    await userEvent.click(expander("D:\\"));
    await waitFor(() => screen.queryAllByRole("button", { name: /ComfyUI-Backup/ }).length > 0);
    await userEvent.click(node("ComfyUI-Backup"));
    await waitFor(() => verdict()?.classList.contains("ok") === true);

    expect(verdict()!.textContent).toContain("This install is on drive D:");
    expect(verdict()!.textContent).toContain("copied");
    expect(verdict()!.textContent).toContain("94 GB");
  });
});

describe("choosing the vault folder", () => {
  it("refuses a folder inside an install, and says why", async () => {
    await mountPicker((h) => openVaultPicker(h.app));
    await userEvent.click(expander("C:\\"));
    await waitFor(() => screen.queryAllByRole("button", { name: /Open ComfyUI-Alpha/ }).length > 0);
    await userEvent.click(expander("ComfyUI-Alpha"));
    await waitFor(() => screen.queryAllByRole("button", { name: /Open models/ }).length > 0);
    await userEvent.click(node("models"));
    await waitFor(() => verdict()?.classList.contains("no") === true);

    expect(verdict()!.textContent).toContain("That is inside an install");
    expect(verdict()!.textContent).toContain("Production");
    expect(confirmButton("Use this folder")).toBeDisabled();
  });

  it("accepts a folder on the same drive as every install", async () => {
    await mountPicker((h) => openVaultPicker(h.app));
    await userEvent.click(expander("C:\\"));
    await waitFor(() => screen.queryAllByRole("button", { name: /Open ComfyVault/ }).length > 0);
    await userEvent.click(node("ComfyVault"));
    await waitFor(() => verdict()?.classList.contains("ok") === true);

    expect(verdict()!.textContent).toContain("Same drive as every install");
    expect(confirmButton("Use this folder")).toBeEnabled();
  });
});

describe("placing a link", () => {
  it("starts inside the installs, never at a drive", async () => {
    const h = await mountPicker(async (x) => {
      const model = x.app.plan()!.orphans[0]!;
      await openLinkPicker(x.app, model.id);
    });
    const dialog = screen.getByRole("dialog");
    expect(dialog.textContent).toContain("Production");
    expect(dialog.textContent).toContain("Normal");
    expect(screen.queryByRole("button", { name: /^D:\\/ })).toBeNull();
    expect(h.app.modal()!.kind).toBe("picker");
  });
});

describe("making a new folder", () => {
  async function openNewFolderInsideVault() {
    const h = await mountPicker((x) => openVaultPicker(x.app));
    await userEvent.click(expander("C:\\"));
    await waitFor(() => screen.queryAllByRole("button", { name: /Open ComfyVault/ }).length > 0);
    await userEvent.click(node("ComfyVault"));
    await waitFor(() => verdict() !== null);
    await userEvent.click(confirmButton("New folder"));
    await waitFor(() => screen.queryAllByLabelText("Name for the new folder").length > 0);
    return h;
  }

  it("is offered only once a folder is chosen", async () => {
    await mountPicker((h) => openVaultPicker(h.app));
    expect(confirmButton("New folder")).toBeDisabled();
    expect(confirmButton("New folder").title).toBe("Pick a folder first");
  });

  it("refuses an empty name and does not ask the engine", async () => {
    const h = await openNewFolderInsideVault();
    let called = false;
    h.app.engine.createFolder = async () => {
      called = true;
      return { ok: false, reason: "invalid_name" };
    };
    await userEvent.click(confirmButton("Create"));
    await waitFor(() => document.querySelector(".tnew-err") !== null);
    expect(document.querySelector(".tnew-err")!.textContent).toContain(
      "Type a name for the folder.",
    );
    expect(called).toBe(false);
  });

  it("refuses a name with a separator in it", async () => {
    const h = await openNewFolderInsideVault();
    let called = false;
    h.app.engine.createFolder = async () => {
      called = true;
      return { ok: false, reason: "invalid_name" };
    };
    await userEvent.type(
      screen.getByLabelText("Name for the new folder"),
      "wan\\new",
    );
    await userEvent.click(confirmButton("Create"));
    await waitFor(() => document.querySelector(".tnew-err") !== null);
    expect(document.querySelector(".tnew-err")!.textContent).toContain(
      'cannot contain \\ / : * ? " < > |',
    );
    expect(called).toBe(false);
  });

  it("refuses a name already used in that folder", async () => {
    const h = await openNewFolderInsideVault();
    let called = false;
    h.app.engine.createFolder = async () => {
      called = true;
      return { ok: false, reason: "exists" };
    };
    await userEvent.type(screen.getByLabelText("Name for the new folder"), "loras");
    await userEvent.click(confirmButton("Create"));
    await waitFor(() => document.querySelector(".tnew-err") !== null);
    expect(document.querySelector(".tnew-err")!.textContent).toContain(
      "There is already a folder called loras here.",
    );
    expect(called).toBe(false);
  });

  it("creates a good name, marks it new, and selects it", async () => {
    const h = await openNewFolderInsideVault();
    await userEvent.type(
      screen.getByLabelText("Name for the new folder"),
      "wan22",
    );
    await userEvent.click(confirmButton("Create"));
    await waitFor(() => document.querySelector(".tnew-tag") !== null);

    const modal = h.app.modal();
    expect(modal?.kind === "picker" && modal.picked).toBe(
      "C:\\ComfyVault\\wan22",
    );
    expect(document.querySelector(".picked")!.textContent).toBe(
      "C:\\ComfyVault\\wan22",
    );
    expect(h.app.toast()?.message).toBe("Created C:\\ComfyVault\\wan22");
  });

  it("shows the engine's own refusal when it will not create the folder", async () => {
    const h = await openNewFolderInsideVault();
    h.app.engine.createFolder = async () => ({ ok: false, reason: "denied" });
    await userEvent.type(
      screen.getByLabelText("Name for the new folder"),
      "blocked",
    );
    await userEvent.click(confirmButton("Create"));
    await waitFor(() => document.querySelector(".tnew-err") !== null);
    expect(document.querySelector(".tnew-err")!.textContent).toContain(
      "Windows refused to create a folder there.",
    );
  });
});
