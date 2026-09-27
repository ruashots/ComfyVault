import { For, Show, createMemo } from "solid-js";

import { Icon } from "~/components/Icon";
import { Header } from "~/components/Shell";
import { countOf, fmt, minutesLeft } from "~/domain/format";
import { installNameOf } from "~/domain/installname";
import { useApp } from "~/state/store";
import type { ScanProgress } from "~/ipc/contract";

/** The three things a scan does, in the order it does them. */
const PHASES: ReadonlyArray<{
  id: ScanProgress["phase"];
  title: string;
  result: (p: ScanProgress) => string;
}> = [
  {
    id: "enumerating",
    title: "List every model file",
    result: (p) => `${countOf(p.filesSeen, "file", "files")} found`,
  },
  {
    id: "hashing",
    title: "Read each file to find the identical ones",
    result: (p) => `${p.filesHashed} of ${p.filesToHash}`,
  },
  {
    id: "finalizing",
    title: "Work out what is a duplicate of what",
    result: () => "almost there",
  },
];

export function ScanScreen() {
  const app = useApp();
  const progress = () => app.scanProgress()!;

  const overall = createMemo(() => {
    const p = progress();
    if (p.phase === "enumerating") {
      return p.filesToHash > 0 ? Math.min(0.2, (p.filesSeen / p.filesToHash) * 0.2) : 0;
    }
    if (p.phase === "finalizing") return 1;
    return 0.2 + (p.bytesToHash > 0 ? (p.bytesHashed / p.bytesToHash) * 0.75 : 0);
  });

  const phaseStates = createMemo(() => {
    const at = PHASES.findIndex((s) => s.id === progress().phase);
    return PHASES.map((phase, i) => ({
      phase,
      state: i < at ? "done" : i === at ? "now" : "wait",
    }));
  });

  return (
    <>
      <Header
        title="Scanning"
        sub={`${app.installs().length} ${app.installs().length === 1 ? "install" : "installs"} · ${minutesLeft(
          Number.isFinite(progress().etaMs as number)
            ? Math.round(progress().etaMs! / 1000)
            : null,
        )}`}
      >
        <button
          class="btn dng"
          onClick={() => void app.engine.cancelScan(progress().scanId)}
        >
          <Icon name="x" size={13} />
          Cancel
        </button>
      </Header>
      <div class="screen">
        <div class="scroll">
          <div class="barhead">
            <span class="lbl">Reading files</span>
            <span class="sp" />
            <span class="pct">{Math.round(overall() * 100)}%</span>
          </div>
          <div class="bar">
            <i style={{ width: `${overall() * 100}%` }} />
          </div>
          <div class="note up">
            {fmt(progress().bytesHashed)} of {fmt(progress().bytesToHash)} &middot;{" "}
            {progress().filesHashed} of {countOf(progress().filesToHash, "file", "files")}
            <Show when={progress().bytesFromCache > 0}>
              {" "}
              &middot; {fmt(progress().bytesFromCache)} already known, not read
              again
            </Show>
          </div>

          <Show when={progress().currentPath}>
            {(path) => (
              <div class="readout up">
                <div>
                  reading &nbsp;<b>{fileNameOf(path())}</b>
                </div>
                <div class="d">{path()}</div>
              </div>
            )}
          </Show>

          <div class="sec secgap">
            <span class="t">Steps</span>
          </div>
          <div class="steps">
            <For each={phaseStates()}>
              {(entry) => (
                <div class="step" classList={{ [entry.state]: true }}>
                  <span class="ic">
                    <Icon
                      name={
                        entry.state === "done"
                          ? "check"
                          : entry.state === "now"
                            ? "arrow"
                            : "dot"
                      }
                      size={12}
                    />
                  </span>
                  <span class="s">{entry.phase.title}</span>
                  <span class="r">
                    {entry.state === "wait"
                      ? "waiting"
                      : entry.phase.result(progress())}
                  </span>
                </div>
              )}
            </For>
          </div>

          <div class="sec secgap">
            <span class="t">Where it is now</span>
          </div>
          <Show
            when={progress().installId}
            fallback={
              <div class="note">
                ComfyVault has to read each file before it can tell two of them
                apart. Nothing is moved and nothing is changed while it reads.
              </div>
            }
          >
            {(id) => (
              <>
                <div class="kv">
                  <span class="k">Install</span>
                  <span class="v">
                    <b>{installNameOf(id(), app.installs())}</b>
                  </span>
                </div>
                <div class="kv">
                  <span class="k">Files found</span>
                  <span class="v">
                    <b>{progress().filesSeen}</b>
                  </span>
                </div>
                <div class="kv">
                  <span class="k">Read so far</span>
                  <span class="v">
                    <b>{fmt(progress().bytesHashed)}</b>
                  </span>
                </div>
                <div class="note up">
                  Nothing moves during a scan. Cancelling leaves the disk exactly
                  as it is, and the files already read stay read.
                </div>
              </>
            )}
          </Show>
        </div>
      </div>
    </>
  );
}

function fileNameOf(path: string): string {
  const at = Math.max(path.lastIndexOf("\\"), path.lastIndexOf("/"));
  return at < 0 ? path : path.slice(at + 1);
}
