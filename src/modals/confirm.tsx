import { For, Show } from "solid-js";

import { Icon } from "~/components/Icon";
import {
  messageOf,
  useApp,
  type AppStore,
  type ConfirmLine,
} from "~/state/store";

export function openConfirm(
  app: AppStore,
  options: {
    title: string;
    cta: string;
    body: ConfirmLine[];
    action: () => Promise<void> | void;
  },
): void {
  app.setModal({
    kind: "confirm",
    title: options.title,
    cta: options.cta,
    body: options.body,
    action: options.action,
    running: false,
    error: null,
  });
}

export function ConfirmModalView() {
  const app = useApp();
  const modal = () => {
    const current = app.modal();
    return current && current.kind === "confirm" ? current : null;
  };

  /**
   * The modal stays open when the engine refuses, and says why. Closing it on a
   * refusal would leave the person with a toast and no idea what to do next.
   */
  const run = async () => {
    const current = modal();
    if (!current || current.running) return;
    app.patchModal((m) => {
      if (m.kind === "confirm") {
        m.running = true;
        m.error = null;
      }
    });
    try {
      await current.action();
      app.setModal(null);
      await app.actions.refresh();
    } catch (error) {
      app.patchModal((m) => {
        if (m.kind === "confirm") {
          m.running = false;
          m.error = messageOf(error);
        }
      });
    }
  };

  return (
    <Show when={modal()}>
      {(current) => (
        <div
          class="veil"
          onClick={(e) => {
            if (e.target === e.currentTarget && !current().running) {
              app.setModal(null);
            }
          }}
        >
          <div
            class="modal sm"
            role="dialog"
            aria-modal="true"
            aria-label={current().title}
          >
            <div class="mh">
              <Icon name="warn" size={14} />
              <h2>{current().title}</h2>
            </div>
            <div class="mb">
              <For each={current().body}>
                {(line) => (
                  <div
                    class="note"
                    style={{
                      "line-height": "1.6",
                      color: "var(--t-body)",
                      "font-size": "11px",
                      "margin-bottom": "8px",
                    }}
                  >
                    <For each={line}>
                      {(part) => (
                        <Show when={part.emph} fallback={<>{part.text}</>}>
                          <span class="emph">{part.text}</span>
                        </Show>
                      )}
                    </For>
                  </div>
                )}
              </For>
              <Show when={current().error}>
                {(message) => (
                  <div class="verdict no" role="alert">
                    <h4>
                      <Icon name="x" size={12} />
                      That did not happen
                    </h4>
                    <p>{message()}</p>
                  </div>
                )}
              </Show>
            </div>
            <div class="mf">
              <span class="sp" />
              <button
                class="btn"
                disabled={current().running}
                onClick={() => app.setModal(null)}
              >
                Cancel
              </button>
              <button
                class="btn dng"
                disabled={current().running}
                onClick={() => void run()}
              >
                {current().running ? "Working…" : current().cta}
              </button>
            </div>
          </div>
        </div>
      )}
    </Show>
  );
}
