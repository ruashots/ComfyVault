import { For, Show } from "solid-js";

import { Icon } from "~/components/Icon";
import { Header } from "~/components/Shell";
import { blockedWhy } from "~/domain/blocked";
import { fmt, fmtN, fmtU, usedPercent } from "~/domain/format";
import { fileNameOf } from "~/domain/view";
import { openConfirm } from "~/modals/confirm";
import { useApp } from "~/state/store";
import type { ApplyState } from "~/ipc/contract";

const HOW_IT_ENDED: Record<ApplyState, string> = {
  completed: "finished",
  completedWithErrors: "finished, with some files left alone",
  cancelled: "stopped when you asked",
  interrupted: "stopped part way",
  reverted: "undone",
};

export function ApplyDone() {
  const app = useApp();
  const run = () => app.lastApply()!;
  const drive = () => app.vault();

  const revert = () => {
    openConfirm(app, {
      title: "Undo this run",
      cta: "Undo the run",
      body: [
        [
          {
            text: "Every file this run moved goes back to the path it came from, and the link left in its place is removed. Drive ",
          },
          { text: app.vaultVolume() },
          { text: " takes back the " },
          { text: fmt(run().bytesFreed), emph: true },
          { text: " this run removed. Nothing else in the vault is touched." },
        ],
        [
          {
            text: "Putting the files back needs room on the drive they came from. ComfyVault checks that first and refuses rather than half-doing it.",
          },
        ],
      ],
      action: async () => {
        await app.engine.revertApply(run().applyId);
      },
    });
  };

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
          <Show when={(app.planView()?.blocked.length ?? 0) > 0}>
            <div class="kv">
              <span class="k w150">Still cannot move</span>
              <span class="v">
                {app.planView()!.blocked.length}{" "}
                {app.planView()!.blocked.length === 1 ? "file" : "files"} &middot;{" "}
                {fmt(app.planView()?.blockedBytes ?? 0)}{" "}
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
              <button class="btn dng" onClick={revert}>
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
