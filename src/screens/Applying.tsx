import { Show } from "solid-js";

import { Icon } from "~/components/Icon";
import { Header } from "~/components/Shell";
import { fmt, secondsLeft } from "~/domain/format";
import { fileNameOf } from "~/domain/view";
import { useApp } from "~/state/store";

const STEP_WORDS: Record<string, string> = {
  verifying: "checking the file has not changed",
  moving: "moving it into the vault",
  linking: "putting a link where it was",
  cleaning: "tidying up",
};

export function ApplyRunning() {
  const app = useApp();
  const progress = () => app.applyProgress()!;
  const volume = () => app.vault()?.volume ?? "C:";
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
            {fmt(progress().bytesMoved)} of {fmt(progress().bytesToMove)} &middot;{" "}
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
