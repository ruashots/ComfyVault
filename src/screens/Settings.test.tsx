import { screen } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";

import { FixtureEngine } from "~/ipc/fixture/engine";
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

describe("the key that is not there", () => {
  it("offers no Civitai key field, because looking a hash up needs none", async () => {
    await mount();
    const text = document.body.textContent ?? "";
    expect(text).not.toMatch(/api key/i);
    expect(document.querySelector('input[type="password"]')).toBeNull();
  });
});

describe("where the Hugging Face cache is read from", () => {
  it("says it is found the way the libraries find it, not as a raw null", async () => {
    const { app } = await mount();
    expect(app.appState()!.settings.huggingFaceCacheDirs).toBeNull();
    const text = document.body.textContent ?? "";
    expect(text).toContain("Hugging Face cache");
    expect(text).toContain("the way the Hugging Face libraries find it themselves");
    expect(text).not.toContain("null");
  });
});
