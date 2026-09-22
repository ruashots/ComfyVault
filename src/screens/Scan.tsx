import { For, Show, createMemo } from "solid-js";

import { Icon } from "~/components/Icon";
import { Header } from "~/components/Shell";
import { fmt, minutesLeft } from "~/domain/format";
import { useApp } from "~/state/store";
import type { ScanProgress, ScanStepId } from "~/ipc/contract";

/** The five things a scan does, in the order it does them. */
const STEPS: ReadonlyArray<{
  id: ScanStepId;
  title: string;
  result: (p: ScanProgress) => string;
}> = [
  {
    id: "read_folders",
    title: "Read the registered folders",
    result: (p) => `${p.instancesRead ?? 0} instances`,
  },
  {
    id: "read_yaml",
    title: "Read extra_model_paths.yaml",
    result: (p) => {
      const folders = p.extraFolders ?? [];
      if (folders.length === 0) return "no extra folders";
      return `${folders.length} extra ${folders.length === 1 ? "folder" : "folders"} · ${folders.join(", ")}`;
    },
  },
  {
    id: "list_files",
    title: "List every model file",
    result: (p) => `${p.filesListed ?? 0} files · ${fmt(p.bytesListed ?? 0)}`,
  },
  {
    id: "hash_files",
    title: "Read each file to find the identical ones",
    result: (p) => `${p.filesHashed} of ${p.filesToHash}`,
  },
  {
    id: "civitai",
    title: "Ask Civitai about each one",
    result: (p) => `${p.civitaiMatched ?? 0} matched`,
  },
];

export function ScanScreen() {
  const app = useApp();
  const progress = () => app.scanProgress()!;

  const stepStates = createMemo(() => {
    const p = progress();
    const current = STEPS.findIndex((s) => s.id === p.currentStep);
    return STEPS.map((step, i) => ({
      step,
      state: i < current ? "done" : i === current ? "now" : "wait",
    }));
  });

  return (
    <>
      <Header
        title="Scanning"
        sub={`${progress().instancesRead ?? app.scan()?.instances.length ?? 0} instances · ${minutesLeft(progress().etaSeconds)}`}
      >
        <button class="btn dng" onClick={() => void app.engine.cancelScan()}>
          <Icon name="x" size={13} />
          Cancel
        </button>
      </Header>
      <div class="screen">
        <div class="scroll">
          <div class="barhead">
            <span class="lbl">Reading files</span>
            <span class="sp" />
            <span class="pct">{Math.round(progress().overall * 100)}%</span>
          </div>
          <div class="bar">
            <i style={{ width: `${progress().overall * 100}%` }} />
          </div>
          <div class="note up">
            {fmt(progress().bytesHashed)} of {fmt(progress().bytesToHash)} &middot;{" "}
            {progress().filesHashed} of {progress().filesToHash} files
          </div>

          <Show when={progress().current}>
            {(current) => (
              <div class="readout up">
                <div>
                  reading &nbsp;<b>{current().name}</b>
                </div>
                <div class="d">{current().path}</div>
              </div>
            )}
          </Show>

          <div class="sec secgap">
            <span class="t">Steps</span>
          </div>
          <div class="steps">
            <For each={stepStates()}>
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
                  <span class="s">{entry.step.title}</span>
                  <span class="r">
                    {entry.state === "wait"
                      ? "waiting"
                      : entry.step.result(progress())}
                  </span>
                </div>
              )}
            </For>
          </div>

          <div class="sec secgap">
            <span class="t">Found so far</span>
          </div>
          <Show
            when={progress().found}
            fallback={
              <div class="note">
                Nothing yet. ComfyVault has to read each file before it can tell
                two of them apart.
              </div>
            }
          >
            {(found) => (
              <>
                <div class="kv">
                  <span class="k">Unique models</span>
                  <span class="v">
                    <b>{found().models}</b>
                  </span>
                </div>
                <div class="kv">
                  <span class="k">Duplicate copies</span>
                  <span class="v">
                    <b>{found().duplicateCopies}</b>
                  </span>
                </div>
                <div class="kv">
                  <span class="k">Reclaimable</span>
                  <span class="v">
                    <b>{fmt(found().reclaimableBytes)}</b>
                  </span>
                </div>
                <For each={app.scan()?.countedNeverMoved ?? []}>
                  {(entry) => (
                    <div class="kv">
                      <span class="k">
                        In {entry.kind === "custom_nodes" ? "custom_nodes" : "HF cache"}
                      </span>
                      <span class="v">
                        {fmt(entry.bytes)}{" "}
                        <span class="faint">counted, never moved</span>
                      </span>
                    </div>
                  )}
                </For>
              </>
            )}
          </Show>
        </div>
      </div>
    </>
  );
}
