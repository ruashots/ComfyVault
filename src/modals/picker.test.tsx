import { screen } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";

import { FixtureEngine } from "~/ipc/fixture/engine";
import {
  PickerModalView,
  openInstallPicker,
  openLinkPicker,
  openVaultPicker,
} from "~/modals/picker";
import { renderWithApp, waitFor, type Harness } from "~/test/render";

let harness: Harness | null = null;

afterEach(() => {
  harness?.unmount();
  harness = null;
});

async function mountPicker(open: (h: Harness) => Promise<void>): Promise<Harness> {
  const engine = new FixtureEngine();
  harness = await renderWithApp(() => <PickerModalView />, { engine });
  await waitFor(() => harness!.app.ready());
  await open(harness);
  await waitFor(() => screen.queryAllByRole("dialog").length > 0);
  return harness;
}

const node = (name: string) =>
  screen.getAllByRole("button").find((b) => b.textContent?.startsWith(name))!;
const expander = (name: string) =>
  screen.getByRole("button", { name: `Open ${name}` });
const verdict = () => document.querySelector(".verdict:not(.wait)");
const button = (label: string) => screen.getByRole("button", { name: label });
const openDrive = async () => {
  await userEvent.click(expander("C:\\"));
  await waitFor(() => screen.queryAllByRole("button", { name: /Open Users/ }).length > 0);
};

describe("the person never types a path", () => {
  it("has no text field until New folder is used", async () => {
    await mountPicker((h) => openInstallPicker(h.app));
    expect(document.querySelectorAll("input")).toHaveLength(0);
  });

  it("starts at the drives and walks down from there", async () => {
    await mountPicker((h) => openInstallPicker(h.app));
    expect(node("C:\\")).toBeInTheDocument();
    expect(node("D:\\")).toBeInTheDocument();

    await openDrive();
    expect(node("ComfyUI-Portable")).toBeInTheDocument();

    await userEvent.click(expander("Users"));
    await waitFor(() => screen.queryAllByRole("button", { name: /Open alex/ }).length > 0);
    await userEvent.click(expander("alex"));
    await waitFor(() => screen.queryAllByRole("button", { name: /Downloads/ }).length > 0);
    expect(node("Downloads")).toBeInTheDocument();
  });

  it("closes a folder again and takes its children with it", async () => {
    await mountPicker((h) => openInstallPicker(h.app));
    await openDrive();
    await userEvent.click(screen.getByRole("button", { name: "Collapse C:\\" }));
    await waitFor(() => screen.queryAllByRole("button", { name: /Open Users/ }).length === 0);
    expect(screen.queryByRole("button", { name: /Open Users/ })).toBeNull();
  });
});

describe("registering an install", () => {
  it("refuses a folder with no ComfyUI inside, and says what to pick", async () => {
    await mountPicker((h) => openInstallPicker(h.app));
    await openDrive();
    await userEvent.click(node("Users"));
    await waitFor(() => verdict()?.classList.contains("no") === true);
    expect(verdict()!.textContent).toContain("Not a ComfyUI install");
    expect(verdict()!.textContent).toContain("the one with main.py in it");
    expect(button("Add this install")).toBeDisabled();
  });

  it("refuses an install that is registered already", async () => {
    await mountPicker((h) => openInstallPicker(h.app));
    await openDrive();
    await userEvent.click(node("ComfyUI-Studio"));
    await waitFor(() => verdict()?.classList.contains("no") === true);
    expect(verdict()!.textContent).toContain("Already registered");
    expect(verdict()!.textContent).toContain("Studio");
    expect(button("Add this install")).toBeDisabled();
  });

  it("says why, in the engine's own words, when a folder will not open", async () => {
    await mountPicker((h) => openInstallPicker(h.app));
    await openDrive();
    await userEvent.click(expander("Program Files"));
    await waitFor(() => document.querySelector(".tnew-err") !== null);
    expect(document.querySelector(".tnew-err")!.textContent).toContain(
      "Windows will not let ComfyVault open that folder.",
    );
    expect(node("Program Files").textContent).toContain("cannot be opened");
  });

  it("accepts a real install and says what it found", async () => {
    await mountPicker((h) => openInstallPicker(h.app));
    await openDrive();
    await userEvent.click(node("ComfyUI-Portable"));
    await waitFor(() => verdict()?.classList.contains("ok") === true);
    expect(verdict()!.textContent).toContain("This is a ComfyUI install");
    expect(verdict()!.textContent).toContain("C:\\ComfyUI-Portable\\models");
    expect(button("Add this install")).toBeEnabled();
  });

  it("warns that an install on another drive is copied, not moved", async () => {
    await mountPicker((h) => openInstallPicker(h.app));
    await userEvent.click(expander("D:\\"));
    await waitFor(() => screen.queryAllByRole("button", { name: /ComfyUI-Backup/ }).length > 0);
    await userEvent.click(node("ComfyUI-Backup"));
    await waitFor(() => verdict()?.classList.contains("ok") === true);
    expect(verdict()!.textContent).toContain("This install is on drive D:");
    expect(verdict()!.textContent).toContain("copied");
  });
});

describe("choosing the vault folder", () => {
  it("refuses a folder inside an install, and says why", async () => {
    await mountPicker((h) => openVaultPicker(h.app));
    await userEvent.click(expander("C:\\"));
    await waitFor(
      () => screen.queryAllByRole("button", { name: /Open ComfyUI-Studio/ }).length > 0,
    );
    await userEvent.click(expander("ComfyUI-Studio"));
    await waitFor(() => screen.queryAllByRole("button", { name: /Open models/ }).length > 0);
    await userEvent.click(node("models"));
    await waitFor(() => verdict()?.classList.contains("no") === true);
    expect(verdict()!.textContent).toContain("That is inside an install");
    expect(verdict()!.textContent).toContain("Studio");
    expect(button("Use this folder")).toBeDisabled();
  });

  it("accepts a folder on the same drive as every install", async () => {
    await mountPicker((h) => openVaultPicker(h.app));
    await userEvent.click(expander("C:\\"));
    await waitFor(() => screen.queryAllByRole("button", { name: /Open ComfyVault/ }).length > 0);
    await userEvent.click(node("ComfyVault"));
    await waitFor(() => verdict()?.classList.contains("ok") === true);
    expect(verdict()!.textContent).toContain("Same drive as every install");
    expect(button("Use this folder")).toBeEnabled();
  });
});

describe("placing a link", () => {
  it("refuses a folder outside every install", async () => {
    const h = await mountPicker(async (x) => {
      await openLinkPicker(x.app, x.app.orphans()[0]!.sha256);
    });
    await userEvent.click(expander("C:\\"));
    await waitFor(() => screen.queryAllByRole("button", { name: /Open Users/ }).length > 0);
    await userEvent.click(node("Users"));
    await waitFor(() => verdict() !== null);
    expect(verdict()!.textContent).toContain("Outside every install");
    expect(button("Put the link here")).toBeDisabled();
    expect(h.app.orphans().length).toBeGreaterThan(0);
  });

  it("refuses a folder ComfyUI never looks in", async () => {
    await mountPicker(async (x) => {
      await openLinkPicker(x.app, x.app.orphans()[0]!.sha256);
    });
    await userEvent.click(expander("C:\\"));
    await waitFor(
      () => screen.queryAllByRole("button", { name: /Open ComfyUI-Studio/ }).length > 0,
    );
    await userEvent.click(expander("ComfyUI-Studio"));
    await waitFor(() => screen.queryAllByRole("button", { name: /custom_nodes/ }).length > 0);
    await userEvent.click(node("custom_nodes"));
    await waitFor(() => verdict() !== null);
    expect(verdict()!.textContent).toContain("ComfyUI does not look here");
    expect(button("Put the link here")).toBeDisabled();
  });

  it("accepts a model folder inside an install", async () => {
    await mountPicker(async (x) => {
      await openLinkPicker(x.app, x.app.orphans()[0]!.sha256);
    });
    await userEvent.click(expander("C:\\"));
    await waitFor(
      () => screen.queryAllByRole("button", { name: /Open ComfyUI-Studio/ }).length > 0,
    );
    await userEvent.click(expander("ComfyUI-Studio"));
    await waitFor(() => screen.queryAllByRole("button", { name: /Open models/ }).length > 0);
    await userEvent.click(expander("models"));
    await waitFor(() => screen.queryAllByRole("button", { name: /^loras/ }).length > 0);
    await userEvent.click(node("loras"));
    await waitFor(() => verdict()?.classList.contains("ok") === true);
    expect(verdict()!.textContent).toContain("Folder accepted");
    expect(button("Put the link here")).toBeEnabled();
  });
});

describe("making a new folder", () => {
  async function openNewFolderInsideVault() {
    const h = await mountPicker((x) => openVaultPicker(x.app));
    await userEvent.click(expander("C:\\"));
    await waitFor(() => screen.queryAllByRole("button", { name: /Open ComfyVault/ }).length > 0);
    await userEvent.click(node("ComfyVault"));
    await waitFor(() => verdict() !== null);
    await userEvent.click(button("New folder"));
    await waitFor(() => screen.queryAllByLabelText("Name for the new folder").length > 0);
    return h;
  }

  it("is offered only once a folder is chosen", async () => {
    await mountPicker((h) => openVaultPicker(h.app));
    expect(button("New folder")).toBeDisabled();
    expect(button("New folder").title).toBe("Pick a folder first");
  });

  it("refuses an empty name and does not ask the engine", async () => {
    const h = await openNewFolderInsideVault();
    let asked = false;
    h.app.engine.createDirectory = async () => {
      asked = true;
      return { path: "", created: true };
    };
    await userEvent.click(button("Create"));
    await waitFor(() => document.querySelector(".tnew-err") !== null);
    expect(document.querySelector(".tnew-err")!.textContent).toContain(
      "Type a name for the folder.",
    );
    expect(asked).toBe(false);
  });

  it("refuses a name with a separator in it", async () => {
    const h = await openNewFolderInsideVault();
    let asked = false;
    h.app.engine.createDirectory = async () => {
      asked = true;
      return { path: "", created: true };
    };
    await userEvent.type(screen.getByLabelText("Name for the new folder"), "wan\\new");
    await userEvent.click(button("Create"));
    await waitFor(() => document.querySelector(".tnew-err") !== null);
    expect(document.querySelector(".tnew-err")!.textContent).toContain(
      'cannot contain \\ / : * ? " < > |',
    );
    expect(asked).toBe(false);
  });

  it("refuses a name already used in that folder", async () => {
    const h = await openNewFolderInsideVault();
    let asked = false;
    h.app.engine.createDirectory = async () => {
      asked = true;
      return { path: "", created: true };
    };
    await userEvent.type(screen.getByLabelText("Name for the new folder"), "loras");
    await userEvent.click(button("Create"));
    await waitFor(() => document.querySelector(".tnew-err") !== null);
    expect(document.querySelector(".tnew-err")!.textContent).toContain(
      "There is already a folder called loras here.",
    );
    expect(asked).toBe(false);
  });

  it("creates a good name, marks it new, and selects it", async () => {
    const h = await openNewFolderInsideVault();
    await userEvent.type(screen.getByLabelText("Name for the new folder"), "wan22");
    await userEvent.click(button("Create"));
    await waitFor(() => document.querySelector(".tnew-tag") !== null);

    const modal = h.app.modal();
    expect(modal?.kind === "picker" && modal.picked).toBe("C:\\ComfyVault\\wan22");
    expect(document.querySelector(".picked")!.textContent).toBe(
      "C:\\ComfyVault\\wan22",
    );
    expect(h.app.toast()?.message).toBe("Created C:\\ComfyVault\\wan22");
  });

  it("shows the engine's own refusal in its own words", async () => {
    const h = await openNewFolderInsideVault();
    h.app.engine.createDirectory = async () => {
      throw { code: "permissionDenied", message: "Windows refused to write there." };
    };
    await userEvent.type(screen.getByLabelText("Name for the new folder"), "blocked");
    await userEvent.click(button("Create"));
    await waitFor(() => document.querySelector(".tnew-err") !== null);
    expect(document.querySelector(".tnew-err")!.textContent).toContain(
      "Windows refused to write there.",
    );
  });
});

describe("an extra_model_paths.yaml the engine could not fully read", () => {
  it("lists each complaint on its own line, not as one run-on sentence", async () => {
    await mountPicker((h) => openInstallPicker(h.app));
    await openDrive();
    await userEvent.click(node("ComfyUI-Portable"));
    await waitFor(() => verdict()?.classList.contains("wait") === false);

    const lines = document.querySelectorAll(".verdict .paths li");
    expect(lines.length).toBeGreaterThan(0);
    expect([...lines].some((l) => l.textContent?.includes("ESCAPED"))).toBe(true);
    // The install is still usable, and the verdict says so.
    const text = verdict()!.textContent ?? "";
    expect(text).toContain("register the install");
    expect(text).toContain("skipped");
    expect(text).not.toContain("could not be read");
  });
});

describe("adding an install that is on the vault's own drive", () => {
  it("does not tell the person its files will be copied", async () => {
    await mountPicker((h) => openInstallPicker(h.app));
    await openDrive();
    await userEvent.click(node("ComfyUI-Portable"));
    await waitFor(() => verdict()?.classList.contains("wait") === false);

    const text = verdict()!.textContent ?? "";
    expect(text).toContain("This is a ComfyUI install");
    // Same drive means a rename. Saying otherwise contradicts the advice the
    // setup screen gives about which drive to put the vault on.
    expect(text).not.toContain("copied");
    expect(text).not.toContain("pays for them first");
    expect(text).not.toContain("free space before it starts");
  });

  it("says the files are copied only when the drives really differ", async () => {
    const harness = await mountPicker((h) => openInstallPicker(h.app));
    // The vault reports its drive the way Windows does, with a separator.
    expect(harness.app.vault()!.volume).toBe("C:\\");
    expect(harness.app.vaultVolume()).toBe("C:");

    await userEvent.click(expander("D:\\"));
    await waitFor(
      () => screen.queryAllByRole("button", { name: /Open ComfyUI-Backup/ }).length > 0,
    );
    await userEvent.click(node("ComfyUI-Backup"));
    await waitFor(() => verdict()?.classList.contains("wait") === false);

    const text = verdict()!.textContent ?? "";
    expect(text).toContain("This install is on drive D:");
    expect(text).toContain("the vault is on C:");
    expect(text).toContain("copied");
    // And never the raw volume the engine reported.
    expect(text).not.toContain("C:\\ pays");
  });
});

describe("a folder made in the picker", () => {
  it("appears as a row when it is made at a drive root", async () => {
    const harness = await mountPicker((h) => openVaultPicker(h.app));
    // The drive itself is the parent, which is where a vault folder goes.
    await userEvent.click(node("C:\\"));
    await waitFor(() => (harness.app.modal() as { picked: string | null }).picked === "C:\\");

    await userEvent.click(button("New folder"));
    await waitFor(() => document.querySelector(".tnew input") !== null);
    await userEvent.type(document.querySelector(".tnew input")!, "ComfyVault2");
    await userEvent.keyboard("{Enter}");
    await waitFor(() => (harness.app.modal() as { newFolder: unknown }).newFolder === null);

    const picked = (harness.app.modal() as { picked: string }).picked;
    expect(picked).toBe("C:\\ComfyVault2");
    const rows = [...document.querySelectorAll(".tnode")].map((el) => el.textContent);
    expect(rows.some((t) => t?.includes("ComfyVault2"))).toBe(true);
  });

  it("brings the new row into view, because the tree scrolls", async () => {
    const seen: string[] = [];
    const original = Element.prototype.scrollIntoView;
    Element.prototype.scrollIntoView = function (this: Element) {
      seen.push(this.getAttribute("data-path") ?? "");
    };
    try {
      const harness = await mountPicker((h) => openVaultPicker(h.app));
      await userEvent.click(node("C:\\"));
      await waitFor(
        () => (harness.app.modal() as { picked: string | null }).picked === "C:\\",
      );
      await userEvent.click(button("New folder"));
      await waitFor(() => document.querySelector(".tnew input") !== null);
      await userEvent.type(document.querySelector(".tnew input")!, "Deep");
      await userEvent.keyboard("{Enter}");
      await waitFor(
        () => (harness.app.modal() as { newFolder: unknown }).newFolder === null,
      );

      // A row that lands below the fold of the tree's scrolling box is a row
      // the person never sees, which is what "it never appeared" was.
      expect(seen).toContain("C:\\Deep");
    } finally {
      Element.prototype.scrollIntoView = original;
    }
  });

  it("appears as a row, marked new, and becomes the selection", async () => {
    const harness = await mountPicker((h) => openVaultPicker(h.app));
    await openDrive();
    await userEvent.click(node("Users"));
    await waitFor(() => harness.app.modal()!.kind === "picker");

    await userEvent.click(button("New folder"));
    await waitFor(() => document.querySelector(".tnew input") !== null);
    await userEvent.type(document.querySelector(".tnew input")!, "ComfyVault");
    await userEvent.keyboard("{Enter}");

    await waitFor(() => (harness.app.modal() as { newFolder: unknown }).newFolder === null);
    const made = "C:\\Users\\ComfyVault";
    // It is the selection and the footer says so.
    expect((harness.app.modal() as { picked: string }).picked).toBe(made);
    // And it is on screen, which is the part that was missing.
    const rows = [...document.querySelectorAll(".tnode")].map((el) => el.textContent);
    expect(rows.some((t) => t?.includes("ComfyVault"))).toBe(true);
    expect(document.querySelector(".tnew-tag")?.textContent?.toLowerCase()).toContain(
      "new",
    );
  });
});
