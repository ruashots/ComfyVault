import { For, Show } from "solid-js";

import { Icon } from "~/components/Icon";
import { Header } from "~/components/Shell";
import { fmt, fmtN, fmtU, usedPercent } from "~/domain/format";
import { openConfirm } from "~/modals/confirm";
import { useApp } from "~/state/store";
import { MovesLog } from "~/screens/Applying";

export function ApplyDone() {
  const app = useApp();
  const run = () => app.lastRun()!;
  const drive = () => app.machine()!.vaultDrive;
  const freeBefore = () => drive().freeBytes - run().bytesFreed;

  const revert = () => {
    openConfirm(app, {
      title: "Undo this run",
      cta: "Undo the run",
      body: [
        [
          {
            text: "Every file this run moved goes back to the path it came from, and the link left in its place is removed. Drive ",
          },
          { text: drive().letter },
          { text: " returns to " },
          { text: fmt(freeBefore()), emph: true },
          { text: " free. Nothing else in the vault is touched." },
        ],
      ],
      action: async () => {
        await app.engine.revert(run().runId);
        await app.actions.refresh();
        app.actions.showToast("Run undone · every file is back where it was");
      },
    });
  };

  return (
    <>
      <Header
        title="Consolidate"
        sub={`finished · ${fmt(run().bytesFreed)} returned`}
      >
        <button
          class="btn"
          onClick={() => void app.engine.openInExplorer(app.machine()!.vaultPath)}
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
                Back on drive {drive().letter}. {run().linksCreated} copies stopped
                taking room.
              </div>
              <div class="l2">
                {fmt(freeBefore())} free before, {fmt(drive().freeBytes)} free now.{" "}
                {usedPercent(drive().totalBytes, drive().freeBytes)}% of the drive
                used.
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
            <span class="k w150">Renamed in the vault</span>
            <span class="v">
              {run().renamedInVault} files, to keep same-named files apart
            </span>
          </div>
          <div class="kv">
            <span class="k w150">Left alone</span>
            <span class="v">
              {run().leftAlone.files} files &middot; {fmt(run().leftAlone.bytes)}{" "}
              <span class="dim">&middot; the reasons are still in the report</span>
            </span>
          </div>

          <Show when={run().skipped.length > 0}>
            <div class="sec secgap">
              <span class="t">
                {run().skipped.length === 1
                  ? "One file was skipped"
                  : `${run().skipped.length} files were skipped`}
              </span>
            </div>
            <For each={run().skipped}>
              {(skipped) => (
                <div class="grp dead">
                  <div class="grp-h static">
                    <span class="cb dead">
                      <Icon name="x" size={9} />
                    </span>
                    <span class="grp-n">{skipped.filename}</span>
                    <span class="grp-s">{fmt(skipped.bytes)}</span>
                  </div>
                  <div class="grp-why wide">
                    <Show
                      when={skipped.reason.kind === "changed_since_report"}
                      fallback={<>This copy could not be moved, so nothing was done to it.</>}
                    >
                      This file was written to after the report was made, so its
                      contents no longer match what the report checked. ComfyVault
                      stopped on it and did nothing to it. Both copies are still
                      where they were.
                    </Show>
                  </div>
                  <div class="grp-fix up">
                    <button
                      class="btn sm pri"
                      onClick={() => void app.engine.startScan()}
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
          <div class="note">
            Every move in this run is listed in{" "}
            <span class="emph">{run().logPath}</span>. Reverting puts each file back
            at the path it came from and removes the link. Files that were already
            links stay as they are.
          </div>
          <MovesLog lines={run().log} limit={9} />
          <div style={{ display: "flex", gap: "8px", "margin-top": "11px" }}>
            <button
              class="btn"
              onClick={() => void app.engine.openInExplorer(run().logPath)}
            >
              <Icon name="file" size={13} />
              Open moves.log
            </button>
            <Show when={run().revertable}>
              <button class="btn dng" onClick={revert}>
                <Icon name="refresh" size={13} />
                Undo this run
              </button>
            </Show>
          </div>
        </div>
      </div>
    </>
  );
}
