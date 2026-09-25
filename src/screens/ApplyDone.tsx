import { For, Show } from "solid-js";

import { Icon } from "~/components/Icon";
import { Header } from "~/components/Shell";
import { blockedWhy, isSkippedByDesign } from "~/domain/blocked";
import { fmt, fmtN, fmtU, usedPercent } from "~/domain/format";
import { fileNameOf } from "~/domain/view";
import { openUndoBox } from "~/modals/undo";
import type { ApplyState } from "~/ipc/contract";
import { useApp } from "~/state/store";

const HOW_IT_ENDED: Record<ApplyState, string> = {
  completed: "finished",
  completedWithErrors: "finished, with some files left alone",
  cancelled: "stopped when you asked",
  interrupted: "stopped part way",
  partlyReverted: "undo stopped part way",
  reverted: "undone",
};

export function ApplyDone() {
  const app = useApp();
  const run = () => app.lastApply()!;
  const drive = () => app.vault();
  /** What the run's own plan said it could not move, and nothing else. */
  const stuck = () =>
    (app.appliedPlan()?.blocked ?? []).filter(
      (row) => !isSkippedByDesign(row.reason),
    );

  return (
    <>
      <Header
        title="Consolidate"
        sub={`${HOW_IT_ENDED[run().state]} · ${fmt(run().bytesFreed)} returned`}
      >
        <button
          class="btn"
          onClick={() =>
            void app.engine.revealInFileManager(app.vault()?.root ?? "")
          }
        >
          <Icon name="folder" size={13} />
          Open the vault folder
        </button>
      </Header>
      <div class="screen">
        <div class="scroll">
          <div class="hero done">
            <div class="big">
              {fmtN(run().bytesFreed)}
              <small>{fmtU(run().bytesFreed)}</small>
            </div>
            <div class="txt">
              <div class="l1">
                Back on drive {app.vaultVolume()}. {run().linksCreated} copies stopped
                taking room.
              </div>
              <div class="l2">
                <Show
                  when={run().vaultFreeBytesBefore !== null && run().vaultFreeBytesAfter !== null}
                  fallback={
                    <>
                      Drive {app.vaultVolume()} did not answer when asked how much room
                      it had, so there is nothing to compare. The figure above is
                      what this run removed.
                    </>
                  }
                >
                  Drive {app.vaultVolume()} had {fmt(run().vaultFreeBytesBefore!)} free
                  before and has {fmt(run().vaultFreeBytesAfter!)} now, both read from
                  the drive itself.{" "}
                  <Show when={drive()?.totalBytes != null}>
                    {usedPercent(drive()!.totalBytes!, run().vaultFreeBytesAfter!)}%
                    of it used.
                  </Show>
                </Show>
              </div>
            </div>
            <Icon name="check" size={26} />
          </div>

          <div class="sec secgap">
            <span class="t">What happened</span>
          </div>
          <div class="kv">
            <span class="k w150">Moved to the vault</span>
            <span class="v">
              <b>{run().filesMoved}</b> files, one copy of each model
            </span>
          </div>
          <div class="kv">
            <span class="k w150">Links created</span>
            <span class="v">
              <b>{run().linksCreated}</b> places, every one at the path it always
              had
            </span>
          </div>
          <div class="kv">
            <span class="k w150">Groups asked for</span>
            <span class="v">
              {run().groupsApplied} of {run().groupsRequested} done
              <Show when={run().groupsFailed > 0}>
                <span class="red"> &middot; {run().groupsFailed} left alone</span>
              </Show>
            </span>
          </div>
          {/* What the plan that ran could not move, read from that plan. A
              plan rebuilt from the same scan reports every consolidated path
              as changed, because it is a link now and the scan saw a file. */}
          <Show when={stuck().length > 0}>
            <div class="kv">
              <span class="k w150">Still cannot move</span>
              <span class="v">
                {stuck().length} {stuck().length === 1 ? "file" : "files"} &middot;{" "}
                {fmt(stuck().reduce((sum, row) => sum + row.sizeBytes, 0))}{" "}
                <span class="dim">&middot; the reasons are still in the report</span>
              </span>
            </div>
          </Show>

          <Show when={run().failures.length > 0}>
            <div class="sec secgap">
              <span class="t">
                {run().failures.length === 1
                  ? "One file was left alone"
                  : `${run().failures.length} files were left alone`}
              </span>
            </div>
            <For each={run().failures}>
              {(failure) => (
                <div class="grp dead">
                  <div class="grp-h static">
                    <span class="cb dead">
                      <Icon name="x" size={9} />
                    </span>
                    <span class="grp-n">{fileNameOf(failure.absPath)}</span>
                  </div>
                  <div class="grp-why wide">
                    {blockedWhy({
                      absPath: failure.absPath,
                      installId: null,
                      installLabel: null,
                      sizeBytes: 0,
                      sha256: null,
                      reason: failure.reason,
                      detail: failure.detail,
                    })}{" "}
                    Nothing was done to it, and both copies are still where they
                    were.
                  </div>
                  <div class="grp-fix up">
                    <button
                      class="btn sm pri"
                      onClick={() =>
                        void app.actions.run(() => app.engine.startScan())
                      }
                    >
                      Run the dry run again
                    </button>
                  </div>
                </div>
              )}
            </For>
          </Show>

          <div class="sec secgap">
            <span class="t">Undo</span>
          </div>
          <Show
            when={run().revertible}
            fallback={
              <div class="note">
                This run can no longer be undone. A later run depends on it, or its
                record is gone.
              </div>
            }
          >
            <div class="note">
              Every step of this run was written down as it happened. Undoing puts
              each file back at the path it came from and removes the link. Files
              that were already links stay as they are.
            </div>
            <div style={{ display: "flex", gap: "8px", "margin-top": "11px" }}>
              <button class="btn dng" onClick={() => void openUndoBox(app, run().applyId)}>
                <Icon name="refresh" size={13} />
                Undo this run
              </button>
            </div>
          </Show>

          <div class="sec secgap plain">
            <span class="t">What to do next</span>
          </div>
          <div class="note">
            Open ComfyUI and load a workflow that uses one of these models. It will
            load from the same path it always did, through the link. If anything
            looks wrong, undo the run above and nothing is lost.
          </div>
          <div style={{ display: "flex", gap: "8px", "margin-top": "11px" }}>
            <button
              class="btn"
              onClick={() => void app.actions.run(() => app.engine.startScan())}
            >
              <Icon name="scan" size={13} />
              Scan again
            </button>
            <button class="btn" onClick={() => app.actions.go("library")}>
              <Icon name="library" size={13} />
              Look at the library
            </button>
          </div>
        </div>
      </div>
    </>
  );
}

