import { Show } from "solid-js";

import { Icon } from "~/components/Icon";
import { Header } from "~/components/Shell";
import { fmt, secondsLeft } from "~/domain/format";
import { fileNameOf } from "~/domain/view";
import type { RevertProgress } from "~/ipc/contract";
import { useApp } from "~/state/store";

const STEP_WORDS: Record<string, string> = {
  verifying: "checking the file has not changed",
  moving: "moving it into the vault",
  linking: "putting a link where it was",
  cleaning: "tidying up",
};

/** The running screen, for whichever operation is running. */
export function ApplyRunning() {
  const app = useApp();
  return (
    <Show when={app.revertProgress()} fallback={<Applying />}>
      {(progress) => <Undoing progress={progress()} />}
    </Show>
  );
}

function Applying() {
  const app = useApp();
  const progress = () => app.applyProgress()!;
  const volume = () => app.vaultVolume();
  const overall = () =>
    progress().groupTotal > 0 ? progress().groupIndex / progress().groupTotal : 0;

  const sameDrive = () =>
    (app.plan()?.totals.crossVolumeGroups ?? 0) === 0;

  return (
    <>
      <Header
        title="Applying"
        sub={
          progress().phase === "preflight"
            ? "checking every file before it touches one"
            : "do not close this window"
        }
      />
      <div class="screen">
        <div class="scroll">
          <div class="barhead">
            <span class="lbl">Moving and linking</span>
            <span class="sp" />
            <span class="pct">{Math.round(overall() * 100)}%</span>
          </div>
          <div class="bar">
            <i style={{ width: `${overall() * 100}%` }} />
          </div>
          <div class="note up">
            {/* The engine counts only the bytes it has to copy across drives.
                A move on one drive is a rename, so on a same-drive run there
                are none, and "0 of 0" is a measurement of nothing. */}
            <Show when={progress().bytesToMove > 0}>
              {fmt(progress().bytesMoved)} of {fmt(progress().bytesToMove)} copied
              across &middot;{" "}
            </Show>
            {progress().groupIndex} of {progress().groupTotal} files &middot;{" "}
            {secondsLeft(
              Number.isFinite(progress().etaMs as number)
            ? Math.round(progress().etaMs! / 1000)
            : null,
              overall(),
            )}
          </div>
          <Show when={sameDrive()}>
            <div class="note" style={{ "margin-top": "4px", color: "var(--t-faint)" }}>
              The vault and every install sit on drive {volume()}, so each file is
              renamed rather than copied. That is why this takes seconds and not
              hours.
            </div>
          </Show>

          <Show when={progress().currentPath}>
            {(path) => (
              <div class="readout up">
                <div>
                  <b>{fileNameOf(path())}</b>
                </div>
                <div class="d">{path()}</div>
                <div class="d">{STEP_WORDS[progress().step] ?? progress().step}</div>
              </div>
            )}
          </Show>

          <div class="sec secgap">
            <span class="t">So far</span>
          </div>
          <div class="step done">
            <span class="ic">
              <Icon name="check" size={12} />
            </span>
            <span class="s">Moved into the vault</span>
            <span class="r">{progress().filesMoved} files</span>
          </div>
          <div class="step done">
            <span class="ic">
              <Icon name="link" size={12} />
            </span>
            <span class="s">Links put back where files were</span>
            <span class="r">{progress().linksCreated} places</span>
          </div>
          <div class="step done">
            <span class="ic">
              <Icon name="vault" size={12} />
            </span>
            <span class="s">Space returned</span>
            <span class="r">{fmt(progress().bytesFreed)}</span>
          </div>
          <Show when={progress().failures > 0}>
            <div class="step bad">
              <span class="ic">
                <Icon name="warn" size={12} />
              </span>
              <span class="s">Stopped on a file, and left it alone</span>
              <span class="r">
                {progress().failures}{" "}
                {progress().failures === 1 ? "file" : "files"}
              </span>
            </div>
          </Show>
          <div class="note up">
            Every step is written down before it happens, so if the power goes this
            run can be finished or undone rather than left half done. A file is
            never deleted before its link is in place. Stopping puts the file it is
            part way through back where it was. Everything already finished stays
            done.
          </div>

          <div style={{ "margin-top": "14px" }}>
            <button
              class="btn dng"
              onClick={() =>
                void app.engine.cancelApply(progress().applyId).then(() =>
                  app.actions.showToast(
                    "Stopping now · everything already finished stays done",
                  ),
                )
              }
            >
              <Icon name="stop" size={13} />
              Stop now
            </button>
          </div>
        </div>
      </div>
    </>
  );
}

/**
 * An undo while it runs.
 *
 * Measured against the real engine, with a program outside the repository
 * that consolidates three installs holding the same nine 32 MB models and then
 * undoes the run. Through the apply shape, the undo reported every counter as
 * zero, counted journal steps (73), and spread the time left evenly over them.
 * In its own shape it reports 27 files back in place, 27 links removed and
 * 576 MB copied back, and the copies were nearly all of its 8 seconds. So the
 * bar and the time left follow the bytes copied back whenever there are any,
 * and journal steps are never shown as a count. The first report arrives
 * before the first step, so this screen replaces the finished one at once.
 */
function Undoing(props: { progress: RevertProgress }) {
  const app = useApp();
  const p = () => props.progress;
  const overall = () => {
    if (p().bytesToCopy > 0) return p().bytesCopied / p().bytesToCopy;
    return p().stepTotal > 0 ? p().stepIndex / p().stepTotal : 0;
  };

  return (
    <>
      <Header title="Undoing" sub="putting every file back · do not close this window" />
      <div class="screen">
        <div class="scroll">
          <div class="barhead">
            <span class="lbl">Putting files back</span>
            <span class="sp" />
            <span class="pct">{Math.round(overall() * 100)}%</span>
          </div>
          <div class="bar">
            <i style={{ width: `${overall() * 100}%` }} />
          </div>
          <div class="note up">
            <Show when={p().bytesToCopy > 0}>
              {fmt(p().bytesCopied)} of {fmt(p().bytesToCopy)} copied back &middot;{" "}
            </Show>
            {p().filesPutBack} of {p().filesToPutBack}{" "}
            {p().filesToPutBack === 1 ? "file" : "files"} back in place &middot;{" "}
            {secondsLeft(
              Number.isFinite(p().etaMs as number) ? Math.round(p().etaMs! / 1000) : null,
              overall(),
            )}
          </div>

          <Show when={p().currentPath}>
            {(path) => (
              <div class="readout up">
                <div>
                  <b>{fileNameOf(path())}</b>
                </div>
                <div class="d">{path()}</div>
                <div class="d">{UNDO_ACTION_WORDS[p().action]}</div>
              </div>
            )}
          </Show>

          <div class="sec secgap">
            <span class="t">So far</span>
          </div>
          <div class="step done">
            <span class="ic">
              <Icon name="check" size={12} />
            </span>
            <span class="s">Files back where they were</span>
            <span class="r">{p().filesPutBack} files</span>
          </div>
          <div class="step done">
            <span class="ic">
              <Icon name="link" size={12} />
            </span>
            <span class="s">Links removed</span>
            <span class="r">{p().linksRemoved} places</span>
          </div>
          <Show when={p().bytesToCopy > 0}>
            <div class="step done">
              <span class="ic">
                <Icon name="vault" size={12} />
              </span>
              <span class="s">Copied back out of the vault</span>
              <span class="r">{fmt(p().bytesCopied)}</span>
            </div>
          </Show>
          <div class="note up">
            The run is undone step by step in reverse, from the record written
            down as it ran. Every file goes back to the path it came from, and
            the vault keeps nothing this run put in it. A copy the run removed is
            copied back out of the vault, and those copies are most of the time
            an undo takes. Stopping leaves the file it is part way through as it
            was, behind its link. Everything already put back stays back.
          </div>

          <div style={{ "margin-top": "14px" }}>
            <button
              class="btn dng"
              onClick={() =>
                void app.engine.cancelApply(p().applyId).then(() =>
                  app.actions.showToast(
                    "Stopping now · everything already put back stays back",
                  ),
                )
              }
            >
              <Icon name="stop" size={13} />
              Stop now
            </button>
          </div>
        </div>
      </div>
    </>
  );
}

const UNDO_ACTION_WORDS: Record<RevertProgress["action"], string> = {
  removingLink: "removing the link",
  renamingBack: "putting it back where it was",
  copyingBack: "copying it back out of the vault",
  tidying: "tidying up",
};
