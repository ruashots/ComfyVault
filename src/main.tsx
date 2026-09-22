import { render } from "solid-js/web";

import { App } from "~/App";
import { createEngine } from "~/ipc/client";
import { AppProvider, createAppStore } from "~/state/store";
import "~/styles/app.css";

const root = document.getElementById("root");
if (!root) throw new Error("the window has no root element");

/**
 * The engine is loaded before the first frame, so no screen ever renders
 * without one. If it cannot be reached the window says so instead of sitting
 * blank, because a blank window tells the person nothing.
 */
createEngine().then(
  (engine) => {
    render(() => {
      const app = createAppStore(engine);
      return (
        <AppProvider value={app}>
          <App />
        </AppProvider>
      );
    }, root);
  },
  (error: unknown) => {
    const detail = error instanceof Error ? error.message : String(error);
    render(
      () => (
        <div class="win">
          <div class="body">
            <div class="main">
              <div class="screen">
                <div class="empty">
                  <h2>ComfyVault could not start</h2>
                  <p>
                    The part of ComfyVault that reads your drive did not load.
                    Close the window and open it again. If it keeps happening,
                    this is what it said.
                  </p>
                  <div class="readout">{detail}</div>
                </div>
              </div>
            </div>
          </div>
        </div>
      ),
      root,
    );
  },
);
