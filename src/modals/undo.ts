import { volumeLabel } from "~/domain/drives";
import { fmt } from "~/domain/format";
import type { RevertDrive, RevertPreview } from "~/ipc/contract";
import { openConfirm } from "~/modals/confirm";
import { detailOf, messageOf, type AppStore, type ConfirmLine } from "~/state/store";

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
export async function openUndoBox(app: AppStore, applyId: string): Promise<void> {
  const lead: ConfirmLine = [
    {
      text: "Every file this run moved goes back to the path it came from, and the link left in its place is removed. Nothing else in the vault is touched.",
    },
  ];
  let preview: RevertPreview;
  try {
    preview = await app.engine.previewRevert(applyId);
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
      await app.engine.revertApply(applyId);
    },
    refusal:
      short.length > 0
        ? {
            head: "There is not enough room to undo this run",
            message: `Free some space on drive ${short
              .map((d) => volumeLabel(d.volume))
              .join(" and ")}, then undo it again. This undo has not started.`,
            detail: [],
          }
        : undefined,
  });
}

const files = (n: number) => `${n} ${n === 1 ? "file" : "files"}`;

/** How the files come back, and what sets the time. */
export function costLine(preview: RevertPreview): ConfirmLine {
  const renamed = preview.filesRenamedBack;
  const copied = preview.filesCopiedBack;
  if (copied === 0) {
    return [
      { text: `${renamed === 1 ? "The file comes" : `All ${renamed} files come`} back at once, by a rename. Nothing has to be copied.` },
    ];
  }
  // A run onto another drive copies even the kept copies back, so there can
  // be nothing that comes back by a rename.
  const instant: ConfirmLine =
    renamed === 0
      ? []
      : [{ text: `${files(renamed)} ${renamed === 1 ? "comes" : "come"} back at once, by a rename. ` }];
  return [
    ...instant,
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
