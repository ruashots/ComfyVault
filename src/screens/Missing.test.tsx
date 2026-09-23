import { render, screen } from "@solidjs/testing-library";
import { afterEach, describe, expect, it } from "vitest";

import { Boundary } from "~/components/Boundary";
import { minutesLeft, secondsLeft } from "~/domain/format";
import { FixtureEngine } from "~/ipc/fixture/engine";
import { SettingsScreen } from "~/screens/Settings";
import { renderWithApp, waitFor, type Harness } from "~/test/render";

let harness: Harness | null = null;

afterEach(() => {
  harness?.unmount();
  harness = null;
});

/**
 * An engine that drops a field on its way out, the way a serialiser does when
 * the two sides spell a name differently. The interface must survive this: a
 * missing field is an absent answer, never a crash and never a blank panel.
 */
function engineMissing(...fields: string[]): FixtureEngine {
  const engine = new FixtureEngine();
  const strip = <T,>(value: T): T => {
    const copy = { ...value } as Record<string, unknown>;
    for (const field of fields) delete copy[field];
    return copy as T;
  };
  const appState = engine.getAppState.bind(engine);
  const settings = engine.getSettings.bind(engine);
  engine.getSettings = async () => strip(await settings());
  engine.getAppState = async () => {
    const state = await appState();
    return { ...state, settings: strip(state.settings) };
  };
  return engine;
}

/** Both names the cache setting can arrive under while the rename is in flight. */
const CACHE_NAMES = ["huggingFaceCacheDirs", "huggingfaceCacheDirs"];

async function mountSettings(engine: FixtureEngine): Promise<Harness> {
  harness = await renderWithApp(() => <SettingsScreen />, { engine });
  await waitFor(() => harness!.app.appState() !== null);
  return harness;
}

describe("a settings field the engine did not send", () => {
  it("still draws every panel below it", async () => {
    await mountSettings(engineMissing(...CACHE_NAMES));
    expect(screen.getByText("What a scan reads")).toBeInTheDocument();
    expect(screen.getByText("Before a copy is deleted")).toBeInTheDocument();
    expect(
      screen.getByRole("switch", { name: /Read both files again/ }),
    ).toBeInTheDocument();
    expect(screen.getByText("The vault's own health")).toBeInTheDocument();
  });

  it("says the field is not known rather than guessing an answer", async () => {
    await mountSettings(engineMissing(...CACHE_NAMES));
    const row = screen.getByText("Hugging Face cache").parentElement!;
    expect(row.textContent).toContain("not known");
    expect(row.textContent).not.toContain("undefined");
    expect(row.textContent).not.toContain("the way the Hugging Face libraries");
  });

  it("keeps a missing switch off rather than letting it read as on", async () => {
    await mountSettings(engineMissing("verifyBeforeDelete"));
    const toggle = screen.getByRole("switch", { name: /Read both files again/ });
    expect(toggle).toHaveAttribute("aria-checked", "false");
    expect(screen.getByText("Before a copy is deleted")).toBeInTheDocument();
  });

  it("draws the list of file types even when the list is missing", async () => {
    await mountSettings(engineMissing("scanExtensions"));
    expect(screen.getByText("What a scan reads")).toBeInTheDocument();
    expect(document.body.textContent).not.toContain("undefined");
  });
});

describe("a time estimate the engine did not send", () => {
  it("never puts NaN in front of a person", () => {
    for (const bad of [undefined, Number.NaN, Number.POSITIVE_INFINITY]) {
      const value = bad as unknown as number;
      expect(minutesLeft(value)).toBe("working");
      expect(secondsLeft(value, 0.5)).not.toContain("NaN");
    }
  });
});

describe("a panel that throws", () => {
  it("says so in the product's own voice instead of going blank", () => {
    const Bad = () => {
      throw new Error("Cannot read properties of undefined (reading 'length')");
    };
    const result = render(() => (
      <Boundary where="What a scan reads">
        <Bad />
      </Boundary>
    ));
    const text = result.container.textContent ?? "";
    expect(text).toContain("ComfyVault could not draw this part of the screen");
    expect(text).toContain("What a scan reads");
    // Nothing has been changed on disk, and the person is told so.
    expect(text).toContain("Nothing on your drive has been changed");
    // The reason is shown, because a blank area tells them nothing.
    expect(text).toContain("reading 'length'");
    result.unmount();
  });

  it("draws its children untouched when nothing throws", () => {
    const result = render(() => (
      <Boundary where="What a scan reads">
        <div>the real panel</div>
      </Boundary>
    ));
    expect(result.container.textContent).toBe("the real panel");
    result.unmount();
  });
});
