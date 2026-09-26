import { screen } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";

import { cacheDirsOf } from "~/ipc/contract";
import { FixtureEngine } from "~/ipc/fixture/engine";
import { ConfirmModalView } from "~/modals/confirm";
import { SettingsScreen } from "~/screens/Settings";
import { renderWithApp, waitFor, type Harness } from "~/test/render";

let harness: Harness | null = null;

afterEach(() => {
  harness?.unmount();
  harness = null;
});

async function mount(): Promise<Harness> {
  const engine = new FixtureEngine();
  harness = await renderWithApp(() => <SettingsScreen />, { engine });
  await waitFor(() => harness!.app.appState() !== null);
  return harness;
}

const verifySwitch = () =>
  screen.getByRole("switch", {
    name: /Read both files again before deleting a duplicate/,
  });

describe("reading a duplicate again before it is deleted", () => {
  it("is on when nobody has touched it", async () => {
    const { app } = await mount();
    expect(app.appState()!.settings.verifyBeforeDelete).toBe(true);
    expect(verifySwitch()).toHaveAttribute("aria-checked", "true");
  });

  it("turns off and stays off, and the engine is the one that remembers", async () => {
    const { app, engine } = await mount();
    await userEvent.click(verifySwitch());
    await waitFor(() => app.appState()!.settings.verifyBeforeDelete === false);
    expect(verifySwitch()).toHaveAttribute("aria-checked", "false");
    expect((await engine.getSettings()).verifyBeforeDelete).toBe(false);

    await userEvent.click(verifySwitch());
    await waitFor(() => app.appState()!.settings.verifyBeforeDelete === true);
    expect((await engine.getSettings()).verifyBeforeDelete).toBe(true);
  });

  it("says what turning it off costs, and does not recommend it", async () => {
    await mount();
    const text = document.body.textContent ?? "";
    expect(text).toContain("cannot be undone");
    expect(text).toContain("a matter of trust rather than proof");
    // No token from the engine ever reaches the glass.
    expect(text).not.toContain("verifyBeforeDelete");
  });
});

describe("the Hugging Face and Civitai tokens", () => {
  const row = (name: string) =>
    [...document.querySelectorAll(".tok")].find(
      (t) => t.querySelector(".lb")?.textContent === name,
    )!;
  const status = (name: string) => row(name).querySelector(".tok-s")!.textContent!.replace(/\s+/g, " ");
  const tokenField = (name: string) =>
    screen.getByLabelText(`${name} token`) as HTMLInputElement;

  async function mountTokens(prepare?: (engine: FixtureEngine) => Promise<void> | void) {
    const engine = new FixtureEngine();
    await prepare?.(engine);
    harness = await renderWithApp(
      () => (
        <>
          <SettingsScreen />
          <ConfirmModalView />
        </>
      ),
      { engine },
    );
    await waitFor(() => document.querySelectorAll(".tok .tok-s").length === 2);
    await waitFor(() => status("Hugging Face").length > 0 && status("Civitai").length > 0);
    return harness;
  }

  it("keeps the lookup free of any key: a key is only for downloads", async () => {
    await mountTokens();
    const lookup = [...document.querySelectorAll(".sec .t")].find(
      (t) => t.textContent === "Civitai lookup",
    )!;
    // The lookup's own switch and note hold no password field.
    let el = lookup.parentElement!.nextElementSibling;
    while (el && !el.classList.contains("sec")) {
      expect(el.querySelector('input[type="password"]')).toBeNull();
      el = el.nextElementSibling;
    }
    expect(document.querySelectorAll('.tok input[type="password"]')).toHaveLength(2);
  });

  it("no longer says the lookup switch takes ComfyVault offline", async () => {
    await mountTokens();
    const text = document.body.textContent!.replace(/\s+/g, " ");
    expect(text).toContain("Turn it off and ComfyVault goes online only to download a model you ask for.");
    expect(text).not.toContain("no network at all");
  });

  it("says where each token is kept, and that a public model needs none", async () => {
    await mountTokens();
    expect(document.body.textContent).toContain(
      "Each token is kept in Windows Credential Manager on this PC, not in the vault folder, so it does not travel with the vault.",
    );
    expect(status("Hugging Face")).toBe(
      "No token. Models that need a signed-in account will not download. Create a read token on huggingface.co, in Settings, under Access Tokens.",
    );
    expect(status("Civitai")).toBe(
      "No token. Models that need a signed-in account will not download. Create a key on civitai.com, in Account settings, under API Keys.",
    );
  });

  it("checks a token before it saves it, and shows the account", async () => {
    const { engine } = await mountTokens();
    await userEvent.type(tokenField("Hugging Face"), "hf_good");
    await userEvent.click(row("Hugging Face").querySelector("button")!);
    await waitFor(() => status("Hugging Face").startsWith("Saved and working."));
    expect(status("Hugging Face")).toBe(
      "Saved and working. Hugging Face knows it as the account example-user. A read token is enough.",
    );
    expect((await engine.getTokenStatus("huggingface")).saved).toBe(true);
    // The field no longer holds the token.
    expect(screen.queryByLabelText("Hugging Face token")).toBeNull();
    expect(screen.getByRole("button", { name: "Replace" })).toBeDefined();
  });

  it("does not save a token the service refuses, and keeps it in the field", async () => {
    const { engine } = await mountTokens();
    await userEvent.type(tokenField("Civitai"), "bad-key");
    await userEvent.click(row("Civitai").querySelector("button")!);
    await waitFor(() => status("Civitai").startsWith("Civitai did not accept"));
    expect(status("Civitai")).toBe(
      'Civitai did not accept this token. It says: "Invalid API key" The token was not saved. Check it and paste it again.',
    );
    expect(tokenField("Civitai").value).toBe("bad-key");
    expect((await engine.getTokenStatus("civitai")).saved).toBe(false);
  });

  it("says when a saved token stops working, in the service's words", async () => {
    await mountTokens(async (e) => {
      await e.setToken("huggingface", "hf_good");
      e.downloads.devRevokeToken("huggingface");
    });
    expect(status("Hugging Face")).toBe(
      'Hugging Face did not accept this token. It says: "Invalid username or password." Paste a new token, or remove this one.',
    );
  });

  it("asks before removing a token", async () => {
    const { engine } = await mountTokens(async (e) => {
      await e.setToken("civitai", "civitai-key");
    });
    expect(status("Civitai")).toBe("Saved and working. Civitai accepted it.");
    await userEvent.click(
      [...row("Civitai").querySelectorAll("button")].find((b) => b.textContent === "Remove")!,
    );
    await waitFor(() => document.querySelector(".modal") !== null);
    expect(document.querySelector(".modal")!.textContent).toContain(
      "The Civitai token is removed from this PC. Models that need it will not download until you add one again.",
    );
    await userEvent.click(screen.getByRole("button", { name: "Remove it" }));
    await waitFor(() => status("Civitai").startsWith("No token."));
    expect((await engine.getTokenStatus("civitai")).saved).toBe(false);
  });
});

describe("where the Hugging Face cache is read from", () => {
  it("says it is found the way the libraries find it, not as a raw null", async () => {
    const { app } = await mount();
    // Read under whichever name the engine used, never one of them directly.
    expect(cacheDirsOf(app.appState()!.settings)).toBeNull();
    const text = document.body.textContent ?? "";
    expect(text).toContain("Hugging Face cache");
    expect(text).toContain("the way the Hugging Face libraries find it themselves");
    expect(text).not.toContain("null");
  });
});

describe("adding an install before a vault exists", () => {
  it("is not offered, and says the vault comes first", async () => {
    harness = await renderWithApp(() => <SettingsScreen />, {
      engine: new FixtureEngine({ empty: true }),
    });
    await waitFor(() => harness!.app.appState() !== null);
    expect(harness.app.hasVault()).toBe(false);
    expect(screen.getByRole("button", { name: /Add an install/ })).toBeDisabled();
    expect(document.body.textContent).toContain("Choose the vault folder first.");
  });

  it("is offered once the vault exists", async () => {
    const { app } = await mount();
    expect(app.hasVault()).toBe(true);
    expect(screen.getByRole("button", { name: /Add an install/ })).toBeEnabled();
  });
});

describe("what the Civitai lookup says it sends", () => {
  it("claims nothing it cannot keep: a web request carries more than the fingerprint", async () => {
    await mount();
    const text = document.body.textContent ?? "";
    expect(text).toContain("The fingerprint goes out. No filename, no path.");
    expect(text).not.toContain("nothing else");
  });
});
