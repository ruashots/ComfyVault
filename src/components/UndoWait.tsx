import { Show } from "solid-js";

import { useApp } from "~/state/store";

/** Why a link or name button is off while a consolidation is being undone. */
export const UNDO_WAIT = "Wait for the undo to finish";

export function UndoWait() {
  const app = useApp();
  return (
    <Show when={app.undoRunning()}>
      <span class="note undo-wait">{UNDO_WAIT}.</span>
    </Show>
  );
}
