import { For, Show } from "solid-js";

import { Icon } from "~/components/Icon";
import { Header } from "~/components/Shell";
import { blockedWhy, isSkippedByDesign } from "~/domain/blocked";
import { volumeLabel } from "~/domain/drives";
import { fmt, fmtN, fmtU, usedPercent } from "~/domain/format";
import { fileNameOf } from "~/domain/view";
import { openConfirm } from "~/modals/confirm";
import type { ApplyState, RevertDrive, RevertPreview } from "~/ipc/contract";
import { detailOf, messageOf, useApp, type ConfirmLine } from "~/state/store";

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
  /** What the run's own plan said it could not move, and nothing else. */
  const stuck = () =>
    (app.appliedPlan()?.blocked ?? []).filter(
      (row) => !isSkippedByDesign(row.reason),
    );

  /**
   * The engine's own cost check comes first, so the box states only what is
   * true: which files come back by a rename, which are copied and how much,
   * and the room each drive is expected to need. The run's `bytesFreed` is
   * not that room: a sparse or compressed model occupies far less than its
   * size, and so does its copy.
   *
   * Measured against the real engine, on three installs holding the same nine
   * models: 9 files renamed back and 18 copied back, which are the run's 9
   * moves and 18 removed duplicates. Asked again after the undo, it refuses
   * with `conflict`, "That run was already undone.", and the box opens with
   * that refusal and no button to confirm.
   */
  const revert = async () => {
    const lead: ConfirmLine = [
      {
        text: "Every file this run moved goes back to the path it came from, and the link left in its place is removed. Nothing else in the vault is touched.",
      },
    ];
    let preview: RevertPreview;
    try {
      preview = await app.engine.previewRevert(run().applyId);
    } catch (error) {
      openConfirm(app, {
        title: "Undo this run",
        cta: null,
        body: [lead],
        action: () => undefined,
        refusal: {
          head: "This run cannot be undone now",
          message: messageOf(error),
          detail: detailOf(error),
        },
      });
      return;
    }
    const short = preview.drives.filter(
      (d) => d.freeBytes !== null && d.freeBytes < d.predictedRoomBytes,
    );
    openConfirm(app, {
      title: "Undo this run",
      cta: short.length > 0 ? null : "Undo the run",
      body: [lead, costLine(preview), ...preview.drives.map(roomLine)],
      action: async () => {
        await app.engine.revertApply(run().applyId);
      },
      refusal:
        short.length > 0
          ? {
              head: "There is not enough room to undo this run",
              message: `Free some space on drive ${short
                .map((d) => volumeLabel(d.volume))
                .join(" and ")}, then undo it again. Nothing has been touched.`,
              detail: [],
            }
          : undefined,
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
              <button class="btn dng" onClick={() => void revert()}>
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

const files = (n: number) => `${n} ${n === 1 ? "file" : "files"}`;

/** How the files come back, and what sets the time. */
function costLine(preview: RevertPreview): ConfirmLine {
  const renamed = preview.filesRenamedBack;
  const copied = preview.filesCopiedBack;
  if (copied === 0) {
    return [
      { text: `${renamed === 1 ? "The file comes" : `All ${renamed} files come`} back at once, by a rename. Nothing has to be copied.` },
    ];
  }
  return [
    { text: `${files(renamed)} ${renamed === 1 ? "comes" : "come"} back at once, by a rename. ` },
    { text: files(copied), emph: true },
    { text: ` ${copied === 1 ? "has" : "have"} to be copied back out of the vault, ` },
    { text: fmt(preview.bytesToCopy), emph: true },
    {
      text: " in all, because the run deleted those copies to free the room. The copying is what takes the time.",
    },
  ];
}

/**
 * A sparse model's copy can occupy a few kilobytes, which the shared sizes
 * round to "0 MB", and that reads as no room at all.
 */
const room = (bytes: number) =>
  bytes > 0 && bytes < 512 * 1024 ? "less than 1 MB" : fmt(bytes);

/** One drive's expected room beside what it has free. */
function roomLine(drive: RevertDrive): ConfirmLine {
  const name = `Drive ${volumeLabel(drive.volume)}`;
  const need: ConfirmLine = [
    { text: `${name} is expected to need ` },
    { text: room(drive.predictedRoomBytes), emph: true },
    { text: " for the copies" },
  ];
  if (drive.freeBytes === null) {
    return [...need, { text: ". It did not answer when asked how much room it has." }];
  }
  return [
    ...need,
    { text: " and has " },
    { text: fmt(drive.freeBytes), emph: true },
    {
      text: drive.freeBytes < drive.predictedRoomBytes ? " free. That is not enough." : " free.",
    },
  ];
}
