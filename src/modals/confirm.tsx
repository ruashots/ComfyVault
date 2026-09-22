import { For, Show } from "solid-js";

import { Icon } from "~/components/Icon";
import { useApp, type AppState, type ConfirmLine } from "~/state/store";

export function openConfirm(
  app: AppState,
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
  });
}

export function ConfirmModalView() {
  const app = useApp();
  const modal = () => {
    const current = app.modal();
    return current && current.kind === "confirm" ? current : null;
  };

  const run = async () => {
    const current = modal();
    if (!current || current.running) return;
    app.patchModal((m) => {
      if (m.kind === "confirm") m.running = true;
    });
    try {
      await current.action();
    } finally {
      app.setModal(null);
    }
  };

  return (
    <Show when={modal()}>
      {(current) => (
        <div
          class="veil"
          onClick={(e) => {
            if (e.target === e.currentTarget) app.setModal(null);
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
            </div>
            <div class="mf">
              <span class="sp" />
              <button class="btn" onClick={() => app.setModal(null)}>
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
