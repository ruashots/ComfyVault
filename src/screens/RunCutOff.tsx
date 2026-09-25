import { For, Show } from "solid-js";

import { Icon } from "~/components/Icon";
import { Header } from "~/components/Shell";
import type { InterruptedApply } from "~/ipc/contract";
import { openConfirm } from "~/modals/confirm";
import { openUndoBox } from "~/modals/undo";
import { useApp } from "~/state/store";

/**
 * A run cut off part way, by a crash or a closed app, with nothing running it
 * now. The running screen promised it could be finished or undone rather than
 * left half done, and this is where that happens.
 *
 * Measured against the real engine, cutting the process off three groups into
 * a run of nine: the run came back as state `running` and listed as
 * interrupted. Finishing it completed the run, and the record then described
 * the whole run: 9 of 9 groups, 9 files moved, 27 links.
 */
export function RunCutOff(props: { run: InterruptedApply }) {
  return (
    <Show when={props.run.blocked} fallback={<Settle run={props.run} />}>
      <Blocked run={props.run} />
    </Show>
  );
}

function Settle(props: { run: InterruptedApply }) {
  const app = useApp();
  return (
    <>
      <Header title="Consolidate" sub="cut off part way" />
      <div class="screen">
        <div class="scroll">
          <div class="blk">
            <h3>
              <Icon name="warn" size={13} />
              A run stopped part way through
            </h3>
            <div class="blkrow">
              <div class="bl">
                <div class="bt">Finish it, or undo it</div>
                <div class="bd">
                  {props.run.description}{" "}
                  {props.run.stepsPending === 0
                    ? "No step is left half done."
                    : `${props.run.stepsPending} ${props.run.stepsPending === 1 ? "step is" : "steps are"} still waiting.`}{" "}
                  Nothing else can start until this is settled.
                </div>
              </div>
              <div class="ba">
                <button
                  class="btn sm pri"
                  onClick={() =>
                    void app.actions.run(
                      () => app.engine.resumeApply(props.run.applyId),
                      "Finishing where it stopped",
                    )
                  }
                >
                  Finish it
                </button>
                <button
                  class="btn sm dng"
                  onClick={() => void openUndoBox(app, props.run.applyId)}
                >
                  Undo it
                </button>
              </div>
            </div>
          </div>
          <div class="note up">
            Every step was written down before it happened, so this run can be
            finished or undone rather than left half done. Finishing moves the rest
            of the files it was asked for. Undoing puts back what it already moved.
          </div>
        </div>
      </div>
    </>
  );
}

/**
 * A cut-off run that names places outside the vault and the registered
 * installs. The engine will neither finish nor undo it, so the one way out is
 * to set it aside, which changes only its record.
 *
 * Measured against the real engine: a run cut off three groups in, then one of
 * its installs removed from ComfyVault. The run came back blocked, naming the
 * nine places in that install. Finish, undo and the undo's cost check were
 * refused with pathOutsideBoundary. Setting it aside left it off the list of
 * runs to settle, and all nine links on disk still reached the vault. With the
 * install registered again the run came back, no longer blocked, and finishing
 * it completed all nine groups.
 */
function Blocked(props: { run: InterruptedApply }) {
  const app = useApp();
  const shown = () => props.run.blockedPaths.slice(0, 3);
  const more = () => props.run.blockedPaths.length - shown().length;

  const setAside = () =>
    openConfirm(app, {
      title: "Set this run aside",
      cta: "Set it aside",
      body: [
        [
          {
            text: "Setting it aside moves nothing on the disk. Every link this run made keeps pointing into the vault. ",
          },
          {
            text: "A model the run was part way through may have neither its file nor a link until the run is finished or undone.",
            emph: true,
          },
        ],
        [
          {
            text: "The run comes back here, to be finished or undone, once the places it names can be reached again.",
          },
        ],
        [
          {
            text: "The usual causes are the vault being opened on a different computer, or an install moved or removed after the run. A vault someone else prepared can cause it too.",
          },
        ],
        [
          {
            text: "The run names these places, which are not in the vault or in any registered install:",
          },
        ],
      ],
      list: props.run.blockedPaths,
      action: async () => {
        await app.engine.setAsideRun(props.run.applyId);
      },
    });

  return (
    <>
      <Header title="Consolidate" sub="cut off part way" />
      <div class="screen">
        <div class="scroll">
          <div class="blk">
            <h3>
              <Icon name="warn" size={13} />
              A run stopped part way through
            </h3>
            <div class="blkrow">
              <div class="bl">
                <div class="bt">ComfyVault cannot finish or undo it</div>
                <div class="bd">
                  {props.run.description} It names{" "}
                  {props.run.blockedPaths.length}{" "}
                  {props.run.blockedPaths.length === 1 ? "place" : "places"} that are
                  not in the vault or in any registered install, so ComfyVault will
                  not touch it.
                </div>
                <ul class="paths" style={{ "margin-top": "7px" }}>
                  <For each={shown()}>{(path) => <li>{path}</li>}</For>
                  <Show when={more() > 0}>
                    <li>and {more()} more</li>
                  </Show>
                </ul>
              </div>
              <div class="ba">
                <button class="btn sm dng" onClick={setAside}>
                  Set it aside
                </button>
              </div>
            </div>
          </div>
          <div class="note up">
            This usually means the vault was opened on a different computer, or an
            install was moved or removed after the run. A vault someone else
            prepared can cause it too, so look at these places before going on.
          </div>
        </div>
      </div>
    </>
  );
}
