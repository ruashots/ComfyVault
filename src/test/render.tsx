import { render } from "@solidjs/testing-library";
import type { JSX } from "solid-js";

import { FixtureEngine } from "~/ipc/fixture/engine";
import { AppProvider, createAppStore, type AppStore } from "~/state/store";

export interface Harness {
  app: AppStore;
  engine: FixtureEngine;
  unmount: () => void;
  container: HTMLElement;
}

/**
 * Mount a piece of the interface over the fixture engine and wait until the
 * first read of the engine has landed, so tests never look at a half-filled
 * screen.
 */
export async function renderWithApp(
  ui: () => JSX.Element,
  options: { engine?: FixtureEngine } = {},
): Promise<Harness> {
  const engine = options.engine ?? new FixtureEngine();
  let app!: AppStore;
  const result = render(() => {
    app = createAppStore(engine);
    return <AppProvider value={app}>{ui()}</AppProvider>;
  });
  await waitFor(() => app.ready());
  return {
    app,
    engine,
    unmount: result.unmount,
    container: result.container as HTMLElement,
  };
}

/** Wait for a condition, checking after every microtask flush. */
export async function waitFor(
  predicate: () => boolean,
  // Generous, because this only bounds a condition that should already be
  // true. Nothing waits out this clock on a healthy run, and a loaded machine
  // must not be the reason a suite goes red.
  timeoutMs = 10_000,
): Promise<void> {
  const started = Date.now();
  while (!predicate()) {
    if (Date.now() - started > timeoutMs) {
      throw new Error("waitFor gave up");
    }
    await new Promise((resolve) => setTimeout(resolve, 5));
  }
}

/** Home is on screen: its tiles, not the sidebar's list of installs. */
export function homeShown(): boolean {
  return [...document.querySelectorAll(".tile .k")].some((k) => k.textContent === "Installs");
}
