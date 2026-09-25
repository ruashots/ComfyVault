import { For, Show } from "solid-js";

import { Icon } from "~/components/Icon";
import {
  detailOf,
  messageOf,
  useApp,
  type AppStore,
  type ConfirmLine,
} from "~/state/store";

export function openConfirm(
  app: AppStore,
  options: {
    title: string;
    /** Null when the engine already refused, so there is nothing to confirm. */
    cta: string | null;
    body: ConfirmLine[];
    /** Paths the person must see before confirming. */
    list?: readonly string[];
    action: () => Promise<void> | void;
    /** Why the action cannot happen, known before the person presses it. */
    refusal?: { head: string; message: string; detail: readonly string[] };
  },
): void {
  app.setModal({
    kind: "confirm",
    title: options.title,
    cta: options.cta,
    body: options.body,
    list: options.list ?? [],
    action: options.action,
    running: false,
    errorHead: options.refusal?.head ?? "That did not happen",
    error: options.refusal?.message ?? null,
    errorDetail: options.refusal?.detail ?? [],
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
        m.errorDetail = [];
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
          m.errorHead = "That did not happen";
          m.error = messageOf(error);
          m.errorDetail = detailOf(error);
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
              <Show when={current().list.length > 0}>
                <ul class="paths" style={{ "margin-bottom": "8px" }}>
                  <For each={current().list}>{(line) => <li>{line}</li>}</For>
                </ul>
              </Show>
              <Show when={current().error}>
                {(message) => (
                  <div class="verdict no" role="alert">
                    <h4>
                      <Icon name="x" size={12} />
                      {current().errorHead}
                    </h4>
                    <p>{message()}</p>
                    <Show when={current().errorDetail.length > 0}>
                      <ul class="paths">
                        <For each={current().errorDetail}>
                          {(line) => <li>{line}</li>}
                        </For>
                      </ul>
                    </Show>
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
                {current().cta ? "Cancel" : "Close"}
              </button>
              <Show when={current().cta}>
                {(cta) => (
                  <button
                    class="btn dng"
                    disabled={current().running}
                    onClick={() => void run()}
                  >
                    {current().running ? "Working…" : cta()}
                  </button>
                )}
              </Show>
            </div>
          </div>
        </div>
      )}
    </Show>
  );
}
