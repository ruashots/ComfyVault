import { Show, createResource } from "solid-js";

import { Icon } from "~/components/Icon";
import { Header } from "~/components/Shell";
import { openUndoBox } from "~/modals/undo";
import { messageOf, useApp } from "~/state/store";

/**
 * A run whose undo stopped part way: it was stopped, it failed, or the app
 * closed while it ran.
 *
 * What is back and what is not is read from the engine's cost check, which
 * reads the disk, so this screen says the same after a restart as it did the
 * moment the undo stopped. The engine puts each file back by one rename onto
 * its link, so every path holds either its real file or a working link, and
 * both load in ComfyUI.
 *
 * Measured against the real engine: three installs holding the same three
 * 512 MB models, the undo stopped in the middle of a copy back out of the
 * vault. The file being copied was still a working link, one file was back,
 * the other seven were links, the run read `partlyReverted`, and the cost
 * check answered 1 back, 3 to rename and 5 to copy, which is all nine. Undoing
 * again put the other eight back.
 */
export function UndoStopped() {
  const app = useApp();
  const run = () => app.lastApply()!;
  const [preview] = createResource(
    () => run().applyId,
    (applyId) => app.engine.previewRevert(applyId),
  );
  const rest = () => {
    const p = preview();
    return p ? p.filesRenamedBack + p.filesCopiedBack : 0;
  };
  const files = (n: number) => `${n} ${n === 1 ? "file" : "files"}`;

  return (
    <>
      <Header title="Consolidate" sub="undo stopped part way" />
      <div class="screen">
        <div class="scroll">
          <Show
            when={!preview.error}
            fallback={
              <div class="verdict no" role="alert">
                <h4>
                  <Icon name="x" size={12} />
                  ComfyVault could not read how far the undo got
                </h4>
                <p>{messageOf(preview.error)}</p>
              </div>
            }
          >
            <Show when={preview()} fallback={<div class="note">Reading what is back…</div>}>
              {(p) => (
                <>
                  <div class="hero">
                    <div class="big">
                      {p().filesAlreadyBack}
                      <small>back</small>
                    </div>
                    <div class="txt">
                      <div class="l1">
                        The undo stopped part way. {files(p().filesAlreadyBack)}{" "}
                        {p().filesAlreadyBack === 1 ? "is" : "are"} back where{" "}
                        {p().filesAlreadyBack === 1 ? "it was" : "they were"}, and{" "}
                        {files(rest())} {rest() === 1 ? "is" : "are"} still in the
                        vault behind {rest() === 1 ? "its link" : "their links"}.
                      </div>
                      <div class="l2">
                        Every path holds either its own file or a link to the vault,
                        so every model still loads in ComfyUI from the path it always
                        had.
                      </div>
                    </div>
                  </div>

                  <div class="sec secgap">
                    <span class="t">Where it stopped</span>
                  </div>
                  <div class="kv">
                    <span class="k w150">Back in place</span>
                    <span class="v">
                      <b>{p().filesAlreadyBack}</b>{" "}
                      {p().filesAlreadyBack === 1 ? "file" : "files"}, each a real file
                      at its original path again
                    </span>
                  </div>
                  <div class="kv">
                    <span class="k w150">Still in the vault</span>
                    <span class="v">
                      <b>{rest()}</b> {rest() === 1 ? "file" : "files"}, each reached
                      through its link
                    </span>
                  </div>
                </>
              )}
            </Show>
          </Show>

          <div class="sec secgap">
            <span class="t">Undo</span>
          </div>
          <div class="note">
            Everything already put back stays back. Every step of this run is still
            written down, so the rest can be undone at any time.
          </div>
          <div style={{ display: "flex", gap: "8px", "margin-top": "11px" }}>
            <button class="btn dng" onClick={() => void openUndoBox(app, run().applyId)}>
              <Icon name="refresh" size={13} />
              Undo the rest
            </button>
            <button
              class="btn"
              onClick={() => void app.actions.run(() => app.engine.startScan())}
            >
              <Icon name="scan" size={13} />
              Scan now
            </button>
          </div>
        </div>
      </div>
    </>
  );
}
