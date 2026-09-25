import { Icon } from "~/components/Icon";
import { Header } from "~/components/Shell";
import type { InterruptedApply } from "~/ipc/contract";
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
