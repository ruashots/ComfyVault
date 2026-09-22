import { For, Show } from "solid-js";

import { Icon } from "~/components/Icon";
import { Header } from "~/components/Shell";
import { clockTime, driveOf, fmt, secondsLeft } from "~/domain/format";
import { useApp } from "~/state/store";
import type { MoveLogLine } from "~/ipc/contract";

/** The last lines the engine wrote to moves.log. */
export function MovesLog(props: { lines: readonly MoveLogLine[]; limit: number }) {
  const tail = () => props.lines.slice(-props.limit);
  return (
    <div class="log">
      <For each={tail()}>
        {(line) => (
          <div class="l">
            <span class="t">{clockTime(line.at)}</span>
            <span
              class="v"
              classList={{
                mv: line.verb === "move",
                ln: line.verb === "link",
                sk: line.verb === "skip",
              }}
            >
              {line.verb}
            </span>
            <span class="p">{line.detail}</span>
          </div>
        )}
      </For>
    </div>
  );
}

export function ApplyRunning() {
  const app = useApp();
  const progress = () => app.applyProgress()!;
  const vaultDrive = () => driveOf(app.machine()!.vaultPath);

  const installsOnVaultDrive = () =>
    (app.scan()?.instances ?? []).every((i) => driveOf(i.path) === vaultDrive());

  return (
    <>
      <Header
        title="Applying"
        sub={
          progress().stopping
            ? "stopping after this file"
            : "do not close this window"
        }
      />
      <div class="screen">
        <div class="scroll">
          <div class="barhead">
            <span class="lbl">Moving and linking</span>
            <span class="sp" />
            <span class="pct">{Math.round(progress().overall * 100)}%</span>
          </div>
          <div class="bar">
            <i style={{ width: `${progress().overall * 100}%` }} />
          </div>
          <div class="note up">
            {fmt(progress().bytesMoved)} of {fmt(progress().bytesTotal)} &middot;{" "}
            {progress().filesMoved} of {progress().filesTotal} files &middot;{" "}
            {secondsLeft(progress().etaSeconds, progress().overall)}
          </div>
          <Show when={installsOnVaultDrive()}>
            <div class="note" style={{ "margin-top": "4px", color: "var(--t-faint)" }}>
              The vault and every install sit on drive {vaultDrive()}, so each file
              is renamed rather than copied. That is why this takes seconds and not
              hours.
            </div>
          </Show>

          <Show when={progress().current}>
            {(current) => (
              <div class="readout up">
                <div>
                  moving &nbsp;<b>{current().name}</b>
                </div>
                <div class="d">
                  from &nbsp;{current().fromInstance} &middot; {current().fromPath}
                </div>
                <div class="d">to &nbsp;&nbsp;&nbsp;{current().toPath}</div>
                <div class="d">
                  link &nbsp;{current().linkInstance} &middot; {current().linkPath}
                </div>
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
          <Show when={progress().skipped.length > 0}>
            <div class="step bad">
              <span class="ic">
                <Icon name="warn" size={12} />
              </span>
              <span class="s">Skipped, the file changed since the report</span>
              <span class="r">
                {progress().skipped.length}{" "}
                {progress().skipped.length === 1 ? "file" : "files"}
              </span>
            </div>
          </Show>
          <div class="note up">
            Every move is written to{" "}
            <span class="emph">{app.machine()!.vaultPath}\moves.log</span> as it
            happens, so this run can be undone even if the power goes.
          </div>

          <div class="sec secgap plain">
            <span class="t">moves.log</span>
            <span class="n">written as it happens</span>
          </div>
          <MovesLog lines={progress().log} limit={7} />

          <div style={{ "margin-top": "14px" }}>
            <Show
              when={!progress().stopping}
              fallback={
                <span class="note">
                  Finishing the file in progress, then stopping.
                </span>
              }
            >
              <button class="btn dng" onClick={() => void app.engine.stopApply()}>
                <Icon name="stop" size={13} />
                Stop after this file
              </button>
            </Show>
          </div>
        </div>
      </div>
    </>
  );
}
