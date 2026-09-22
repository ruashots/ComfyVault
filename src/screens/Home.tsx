import { For, Show, createMemo } from "solid-js";

import { Icon, Mark } from "~/components/Icon";
import { DanglingLinks } from "~/components/DanglingLinks";
import { Header, Warnbar } from "~/components/Shell";
import { fmt, fmtN, fmtU, relativeTime, usedPercent } from "~/domain/format";
import { openInstallPicker } from "~/modals/picker";
import { useApp } from "~/state/store";
import { ScanScreen } from "~/screens/Scan";

export function HomeScreen() {
  const app = useApp();
  return (
    <Show when={app.hasInstalls()} fallback={<FirstRun />}>
      <Show when={!app.scanProgress()} fallback={<ScanScreen />}>
        <HomeReport />
      </Show>
    </Show>
  );
}

function FirstRun() {
  const app = useApp();
  return (
    <>
      <Header title="Home" sub="no instances yet" />
      <div class="screen">
        <div class="empty">
          <Mark size={34} />
          <h2>Nothing registered yet</h2>
          <p>
            ComfyVault needs to know where your ComfyUI installs are. It reads each
            one, then holds one copy of every model in a single folder and leaves a
            link behind in every place a file used to be.
          </p>
          <div class="acts">
            <button class="btn pri" onClick={() => void openInstallPicker(app)}>
              <Icon name="folder" size={13} />
              Choose an install folder
            </button>
          </div>
          <div class="foot">
            The vault will be created at{" "}
            <span class="emph">{app.vault()?.root ?? "C:\\ComfyVault"}</span>.
            <br />
            Change that in Settings before the first scan.
          </div>
        </div>
      </div>
    </>
  );
}

function HomeReport() {
  const app = useApp();
  const totals = () => app.scan()?.totals ?? null;
  const drive = () => app.vault();
  const run = () => app.lastApply();
  const counted = () => app.planView()?.countedNeverMoved ?? [];

  const afterFree = createMemo(() => {
    const free = drive()?.freeBytes ?? 0;
    return free + (run() ? 0 : (app.plan()?.totals.bytesFreed ?? 0));
  });

  return (
    <>
      <Header
        title="Home"
        sub={
          app.scan()
            ? `last scan ${relativeTime(app.scan()!.finishedAt)}`
            : "not scanned yet"
        }
      >
        <button
          class="btn pri"
          onClick={() => void app.actions.run(() => app.engine.startScan())}
        >
          <Icon name="scan" size={13} />
          Scan now
        </button>
      </Header>
      <div class="screen">
        <Warnbar />
        <div class="scroll">
          <DanglingLinks />
          <Show when={totals()} fallback={<NotScannedYet />}>
            {(t) => (
              <>
                <div class="tiles">
                  <Tile
                    value={String(app.installs().length)}
                    label="Instances"
                    note={app.installs().map((i) => i.label).join(" \u00b7 ")}
                  />
                  <Tile
                    value={String(t().uniqueContents)}
                    label="Unique models"
                    note={`${t().movableFiles} files on disk`}
                  />
                  <Tile
                    value={fmtN(t().movableBytes)}
                    unit={fmtU(t().movableBytes)}
                    label="Models on disk"
                    note={`${fmt(t().uniqueBytes)} if kept once`}
                  />
                  <Tile
                    value={
                      app.usage().size > 0 ? String(app.unusedCount()) : "\u2014"
                    }
                    label="Not used"
                    note={
                      app.usage().size > 0
                        ? "name not found in any workflow"
                        : "no workflow files were read"
                    }
                  />
                </div>

                <Show when={run()} fallback={<PlanHero afterFree={afterFree()} />}>
                  {(finished) => (
                    <div class="hero done">
                      <div class="big">
                        {fmtN(finished().bytesFreed)}
                        <small>{fmtU(finished().bytesFreed)}</small>
                      </div>
                      <div class="txt">
                        <div class="l1">
                          Freed. {finished().linksCreated} copies are now links.
                        </div>
                        <div class="l2">
                          Drive {drive()?.volume} has {fmt(drive()?.freeBytes ?? 0)}{" "}
                          free.
                        </div>
                      </div>
                      <button class="btn" onClick={() => app.actions.go("consolidate")}>
                        Open the report
                      </button>
                    </div>
                  )}
                </Show>
              </>
            )}
          </Show>

          <div class="sec secgap">
            <span class="t">Instances</span>
            <Show when={totals()}>
              {(t) => (
                <span class="n">
                  {fmt(t().movableBytes)} of models across {app.installs().length}{" "}
                  installs
                </span>
              )}
            </Show>
          </div>
          <For each={app.installViews()}>
            {(view) => (
              <div class="inst">
                <span class="led" classList={{ up: view.running, idle: !view.running }} />
                <span class="name nm">{view.install.label}</span>
                <span class="path pp">{view.install.root}</span>
                <Show when={view.install.extraPaths.length > 0}>
                  <span class="faint yaml">+ extra_model_paths.yaml</span>
                </Show>
                <span class="num c1">{view.files}</span>
                <span class="num c2">{fmt(view.bytes)}</span>
                <Show when={view.running} fallback={<span class="pill link">idle</span>}>
                  <span class="pill up">running</span>
                </Show>
              </div>
            )}
          </For>

          <Show when={app.installViews().some((v) => v.thumbnailsAffected)}>
            <div class="note up">
              {app
                .installViews()
                .filter((v) => v.thumbnailsAffected)
                .map((v) => v.install.label)
                .join(" and ")}{" "}
              runs ComfyUI 0.28.0 or newer, which will not show a preview thumbnail
              for a model reached through a link. Loading the model and running a
              workflow are not affected.
            </div>
          </Show>

          <div class="sec secgap">
            <span class="t">Recent</span>
          </div>
          <For each={recentLines(app)}>
            {(line) => (
              <div class="act">
                <span class="a">{line.event}</span>
                <span class="d">{line.detail}</span>
                <span class="w">{relativeTime(line.when)}</span>
              </div>
            )}
          </For>

          <Show when={counted().length > 0}>
            <div class="sec secgap">
              <span class="t">Counted, never moved</span>
              <span class="n">
                {fmt(counted().reduce((s, c) => s + c.bytes, 0))}
              </span>
            </div>
            <For each={counted()}>
              {(entry) => (
                <div class="kv">
                  <span class="k w210">
                    {entry.kind === "custom_nodes"
                      ? "Inside custom_nodes"
                      : "Hugging Face cache"}
                  </span>
                  <span class="v">
                    <b>{fmt(entry.bytes)}</b>{" "}
                    <span class="dim">&middot; {entry.files} files</span>
                  </span>
                </div>
              )}
            </For>
            <div class="note up">
              A node can load a weight straight out of either place. ComfyVault
              reports what is there and leaves both alone.
            </div>
          </Show>
        </div>
      </div>
    </>
  );
}

function NotScannedYet() {
  const app = useApp();
  return (
    <div class="hero">
      <div class="big">&mdash;</div>
      <div class="txt">
        <div class="l1">
          {app.installs().length}{" "}
          {app.installs().length === 1 ? "install is" : "installs are"} registered
          and nothing has been read yet.
        </div>
        <div class="l2">
          A scan reads every model file once to find the identical ones. Nothing
          moves until you say so.
        </div>
      </div>
      <button
        class="btn pri"
        onClick={() => void app.actions.run(() => app.engine.startScan())}
      >
        <Icon name="scan" size={13} />
        Scan now
      </button>
    </div>
  );
}

function PlanHero(props: { afterFree: number }) {
  const app = useApp();
  const totals = () => app.plan()?.totals ?? null;
  const drive = () => app.vault();
  return (
    <Show when={totals()}>
      {(t) => (
        <div class="hero">
          <div class="big">
            {fmtN(t().bytesFreed)}
            <small>{fmtU(t().bytesFreed)}</small>
          </div>
          <div class="txt">
            <Show
              when={t().groupsFreeingSpace > 0}
              fallback={
                <>
                  <div class="l1">Every model is held once already.</div>
                  <div class="l2">
                    Consolidating moves them into the vault and leaves a link
                    behind, so nothing on drive {drive()?.volume} changes size.
                  </div>
                </>
              }
            >
              <div class="l1">
                {app.scan()?.totals.duplicateFiles ?? 0} copies of{" "}
                {t().groupsFreeingSpace} models are held twice or more.
              </div>
              <div class="l2">
                Consolidating leaves {fmt(props.afterFree)} free on drive{" "}
                {drive()?.volume},{" "}
                {usedPercent(drive()?.totalBytes ?? 0, props.afterFree)}% used.
              </div>
            </Show>
          </div>
          <button class="btn pri" onClick={() => app.actions.go("consolidate")}>
            Review the plan
            <Icon name="arrow" size={13} />
          </button>
        </div>
      )}
    </Show>
  );
}

interface RecentLine {
  event: string;
  detail: string;
  when: string;
}

/**
 * What has happened lately, from the facts the engine keeps: the last scan,
 * every run, and when each install was registered.
 */
function recentLines(app: ReturnType<typeof useApp>): RecentLine[] {
  const lines: RecentLine[] = [];
  const scan = app.scan();
  if (scan) {
    lines.push({
      event: scan.cancelled ? "vault.scan.cancelled" : "vault.scan.complete",
      detail: `${scan.totals.uniqueContents} models \u00b7 ${scan.totals.duplicateFiles} duplicate copies \u00b7 ${fmt(scan.totals.reclaimableBytes)} reclaimable`,
      when: scan.finishedAt,
    });
  }
  const run = app.lastApply();
  if (run?.finishedAt) {
    lines.push({
      event: `vault.apply.${run.state}`,
      detail: `${run.filesMoved} files moved \u00b7 ${run.linksCreated} links \u00b7 ${fmt(run.bytesFreed)} freed`,
      when: run.finishedAt,
    });
  }
  for (const install of app.installs()) {
    lines.push({
      event: "instance.add",
      detail: `${install.label} \u00b7 ${install.root}`,
      when: install.addedAt,
    });
  }
  const vault = app.vault();
  if (vault) {
    lines.push({
      event: "vault.create",
      detail: `${vault.root} \u00b7 drive ${vault.volume}`,
      when: vault.createdAt,
    });
  }
  return lines
    .sort((a, b) => new Date(b.when).getTime() - new Date(a.when).getTime())
    .slice(0, 6);
}

function Tile(props: {
  value: string;
  unit?: string;
  label: string;
  note: string;
}) {
  return (
    <div class="tile">
      <div class="v">
        {props.value}
        <Show when={props.unit}>
          <small>{props.unit}</small>
        </Show>
      </div>
      <div class="k">{props.label}</div>
      <div class="x">{props.note}</div>
    </div>
  );
}
