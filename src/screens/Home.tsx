import { For, Show, createMemo, createSignal, type JSX } from "solid-js";

import { Icon, type IconName } from "~/components/Icon";
import { Logo } from "~/components/Logo";
import { DanglingLinks } from "~/components/DanglingLinks";
import { ThumbnailNote } from "~/components/ThumbnailNote";
import { Header, Warnbar } from "~/components/Shell";
import {
  countOf,
  driveOf,
  fmt,
  fmtN,
  fmtU,
  mid,
  relativeTime,
  usedPercent,
} from "~/domain/format";
import { driveFor, volumeLabel } from "~/domain/drives";
import { openInstallPicker, openVaultPicker } from "~/modals/picker";
import { processLine, processTooltip, processesFor } from "~/domain/running";
import { installName } from "~/domain/installname";
import { installsTotalsOf } from "~/domain/view";
import { cutOffLine } from "~/domain/download";
import { messageOf, useApp } from "~/state/store";
import type { ApplyState, DriveInfo, Install } from "~/ipc/contract";
import { ScanScreen } from "~/screens/Scan";

export function HomeScreen() {
  const app = useApp();
  return (
    <Show when={app.setupDone()} fallback={<Setup />}>
      <Show when={!app.scanProgress()} fallback={<ScanScreen />}>
        <HomeReport />
      </Show>
    </Show>
  );
}

/**
 * The two things a person sets before anything else works, each ticked when it
 * is done, and what happens after they are.
 *
 * The vault comes first because the engine cannot record an install without
 * one, and because a tick that only lives in this window would be a promise
 * the disk is not keeping. Nothing here states a size: a size comes from a
 * scan, and a scan needs the vault that step one is choosing.
 *
 * Setup stays until the person starts the first scan, so every install goes
 * into one list and one scan reads them all. The scan button in the header is
 * the only way out.
 */
function Setup() {
  const app = useApp();
  const vault = () => app.vault();
  const hasInstalls = () => app.hasInstalls();
  const hasVault = () => app.hasVault();
  const count = () => app.installs().length;
  const vaultDrive = () =>
    vault() ? driveFor(vault()!.root, app.drives()) : null;
  const [starting, setStarting] = createSignal(false);

  const vaultSub = () => {
    const info = vault();
    if (!info) return "not chosen yet";
    const drive = vaultDrive();
    return drive && drive.freeBytes !== null
      ? `${info.root}  ·  ${fmt(drive.freeBytes)} free on ${letterOf(drive)}`
      : info.root;
  };

  const installSub = () =>
    hasInstalls()
      ? `${count()} registered`
      : hasVault()
        ? "none registered yet"
        : "waiting for the vault folder";

  const sub = () =>
    !hasVault()
      ? "nothing set yet"
      : !hasInstalls()
        ? "1 of 2 done"
        : "both set · scan when every install is in the list";

  const scan = async () => {
    if (starting()) return;
    setStarting(true);
    try {
      await app.actions.run(() => app.engine.startScan());
    } finally {
      setStarting(false);
    }
  };

  const remove = async (install: Install) => {
    try {
      await app.engine.unregisterInstall(install.id);
    } catch (error) {
      app.actions.showToast(messageOf(error), "bad");
      return;
    }
    app.setFreshInstalls(app.freshInstalls().filter((root) => root !== install.root));
    await app.actions.refresh();
    app.actions.showToast(
      `Removed ${install.root} from the list · nothing in it was touched`,
    );
  };

  return (
    <>
      <Header title="Setup" sub={sub()}>
        <button
          class="btn"
          classList={{ pri: hasVault() && hasInstalls() }}
          disabled={
            !hasVault() || !hasInstalls() || starting() || app.appState()?.busy != null
          }
          title={
            !hasVault()
              ? "Choose the vault folder first"
              : !hasInstalls()
                ? "Add an install first"
                : undefined
          }
          onClick={() => void scan()}
        >
          <Icon name="scan" size={13} />
          {hasVault() && hasInstalls() ? scanLabel(count()) : "Scan"}
        </button>
      </Header>
      <div class="screen">
        <div class="scroll">
          <div style={{ "text-align": "center", padding: "6px 0 0" }}>
            <Logo height={48} />
            <div
              class="lbl"
              style={{ "margin-top": "8px", "letter-spacing": "1.8px" }}
            >
              Set ComfyVault up
            </div>
            <div
              class="note"
              style={{ "max-width": "470px", margin: "7px auto 0" }}
            >
              One copy of every model in a single folder, and a link left behind
              in every place a file used to be. ComfyUI goes on reading them from
              the paths it already uses.
            </div>
          </div>

          <div class="sec secgap">
            <span class="t">Two things to set</span>
          </div>

          <StepRow
            number={1}
            title="Where the vault goes"
            sub={vaultSub()}
            done={hasVault()}
          >
            <button
              class="btn"
              classList={{ pri: !hasVault() }}
              onClick={() => void openVaultPicker(app)}
            >
              <Icon name="folder" size={13} />
              {hasVault() ? "Change" : "Choose the vault folder"}
            </button>
          </StepRow>

          <div class="setup col" classList={{ done: hasInstalls() }}>
            <div class="shd">
              <span class="sn">
                <Show when={hasInstalls()} fallback={2}>
                  <Icon name="check" size={12} />
                </Show>
              </span>
              <div style={{ flex: 1, "min-width": 0 }}>
                <div class="stt">Your ComfyUI installs</div>
                <div class="sts">{installSub()}</div>
              </div>
              <Show
                when={hasVault()}
                fallback={
                  <button
                    class="btn"
                    disabled
                    title="The vault has to exist before an install can point into it"
                  >
                    <Icon name="folder" size={13} />
                    Choose an install folder
                  </button>
                }
              >
                <button
                  class="btn"
                  classList={{ pri: !hasInstalls() }}
                  onClick={() => void openInstallPicker(app)}
                >
                  <Icon name={hasInstalls() ? "plus" : "folder"} size={13} />
                  {hasInstalls() ? "Add another" : "Choose an install folder"}
                </button>
              </Show>
            </div>
            <For each={app.installs()}>
              {(install) => (
                <>
                  <div
                    class="sreg"
                    classList={{ fresh: app.freshInstalls().includes(install.root) }}
                  >
                    <Icon name="folder" size={12} />
                    <span class="nm" title={install.root}>
                      {installName(install, app.installs())}
                    </span>
                    <span class="pp" title={install.root}>
                      {mid(install.root, 64)}
                    </span>
                    <Show when={install.extraPaths.length > 0}>
                      <span class="tg">+ extra_model_paths.yaml</span>
                    </Show>
                    <span class="tg">drive {driveOf(install.root)}</span>
                    <button
                      class="btn sm"
                      aria-label={`Remove ${install.root} from the list`}
                      onClick={() => void remove(install)}
                    >
                      Remove
                    </button>
                  </div>
                  <For each={processesFor(install.id, app.running())}>
                    {(p) => (
                      <div class="srun">
                        <span class="led up" />
                        <span>
                          running now &middot; {processLine(p)} &middot; scanning
                          works, Apply waits until it is closed
                        </span>
                      </div>
                    )}
                  </For>
                </>
              )}
            </For>
            <Show when={hasVault() && !hasInstalls()}>
              <div class="sempty">
                Add every ComfyUI install on this computer. The folder picker stays
                open, so you can add one after another.
              </div>
            </Show>
          </div>
          <Show when={hasInstalls()}>
            <div class="note up">
              Add every install before you scan. One scan reads them all together,
              which is how it finds the copies they share. When the list is
              complete, press <span class="emph">{scanLabel(count())}</span>.
            </div>
          </Show>

          <div class="note up">
            <Show
              when={hasVault()}
              fallback={
                <>
                  Put the vault on the same drive as your ComfyUI installs. Files
                  are moved there rather than copied, so the vault needs no free
                  space of its own and the room comes back as it goes. On any
                  other drive every file is copied across first, so that drive
                  needs the room up front.
                </>
              }
            >
              The vault is on {app.vaultVolume()}. An install on {app.vaultVolume()}{" "}
              has its files moved, which is instant and needs no spare room. An
              install on any other drive has them copied across instead, and
              ComfyVault says what that needs before anything happens.
            </Show>
          </div>
          <Show when={!hasVault()}>
            <div class="note" style={{ "margin-top": "7px" }}>
              Not a removable or network drive. Every install points into the
              vault by link, so on any day that drive is missing, every model in
              every install stops loading at once.
            </div>
          </Show>

          <div class="sec secgap">
            <span class="t">What happens after this</span>
            <span class="n">nothing moves on its own</span>
          </div>
          <AfterStep icon="scan" title="A scan reads every model file">
            It opens nothing and moves nothing. It reads each file to work out
            which of them are the same file under different names.
          </AfterStep>
          <AfterStep icon="file" title="You get a plan to read">
            Every file it would move, where it would go, what it gives back, and
            anything it cannot touch, with the reason.
          </AfterStep>
          <AfterStep icon="check" title="Nothing moves until you press Apply">
            Each move is written to a log as it happens, so the whole run can be
            undone afterwards.
          </AfterStep>
        </div>
      </div>
    </>
  );
}

/** "Scan 1 install", "Scan 4 installs". */
function scanLabel(n: number): string {
  return `Scan ${n} ${n === 1 ? "install" : "installs"}`;
}

/** "C:" from a drive's root, which the engine writes as "C:\\". */
function letterOf(drive: DriveInfo): string {
  return drive.root.replace(/\\+$/, "");
}

function StepRow(props: {
  number: number;
  title: string;
  sub: string;
  done: boolean;
  children: JSX.Element;
}) {
  return (
    <div class="setup" classList={{ done: props.done }}>
      <span class="sn">
        <Show when={props.done} fallback={props.number}>
          <Icon name="check" size={12} />
        </Show>
      </span>
      <div style={{ flex: 1, "min-width": 0 }}>
        <div class="stt">{props.title}</div>
        <div class="sts">{props.sub}</div>
      </div>
      {props.children}
    </div>
  );
}

function AfterStep(props: {
  icon: IconName;
  title: string;
  children: JSX.Element;
}) {
  return (
    <div class="after">
      <span class="an">
        <Icon name={props.icon} size={13} />
      </span>
      <div>
        <div class="at">{props.title}</div>
        <div class="as">{props.children}</div>
      </div>
    </div>
  );
}

function HomeReport() {
  const app = useApp();
  const totals = () => app.scan()?.totals ?? null;
  const drive = () => app.vault();
  const run = () => app.runOnScreen();
  const counted = () => app.planView()?.countedNeverMoved ?? [];
  const held = createMemo(() => installsTotalsOf(app.contents()));

  /**
   * What the drive would have free once the plan runs. Null when the drive did
   * not say what it has now, because there is nothing to add the saving to.
   */
  const afterFree = createMemo(() => {
    const free = drive()?.freeBytes;
    if (free == null) return null;
    return free + (run() ? 0 : (app.plan()?.totals.bytesFreed ?? 0));
  });

  return (
    <>
      <Header
        title="Home"
        sub={
          app.nothingRead()
            ? "not scanned yet"
            : app.scanPredatesRun()
              ? "not scanned since the run"
              : app.scanPredatesUndo()
                ? "not scanned since the undo"
                : `last scan ${relativeTime(app.scan()!.finishedAt)}`
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
          <Show when={!app.nothingRead() && totals()} fallback={<NotScannedYet />}>
              <>
                <Show when={app.scanPredatesRun()}>
            <div class="note" style={{ "margin-bottom": "9px" }}>
              These figures come from the scan taken before the run, so they
              describe the installs as they were then. Scan again to see what is
              on disk now.
            </div>
          </Show>
                <Show when={app.scanPredatesUndo()}>
            <div class="note" style={{ "margin-bottom": "9px" }}>
              These figures come from a scan taken before the undo put the files
              back, so they do not describe the installs as they are now. Scan
              again to see what is on disk now.
            </div>
          </Show>
          <div class="tiles">
                  <Tile
                    value={String(app.installs().length)}
                    label="Installs"
                    note={app.installs().map((i) => installName(i, app.installs())).join(" · ")}
                  />
                  <Tile
                    value={String(held().stillOut.models)}
                    label="Models still in installs"
                    note={
                      held().stillOut.models === 0 && app.libraryTotal() > 0
                        ? "every model is in the Library"
                        : `${countOf(held().stillOut.files, "file", "files")}, ${fmt(held().stillOut.bytes)}, not in the vault yet`
                    }
                  />
                  <Tile
                    value={String(app.libraryTotal())}
                    label="Models in the Library"
                    note={
                      app.libraryTotal() === 0
                        ? "nothing in the vault yet"
                        : `${fmt(app.library().reduce((sum, row) => sum + row.sizeBytes, 0))} in the vault`
                    }
                  />
                  {/* Measured against the real engine: with no saved workflow
                      file there is nothing to search, and every model comes
                      back as not found and not searched. Counting those as
                      zero unused says every model is in use, which nobody
                      knows. */}
                  <Tile
                    value={
                      app.usage().size === 0
                        ? "not yet"
                        : app.nothingSearched()
                          ? "not known"
                          : String(app.unusedCount())
                    }
                    label="Not used"
                    note={
                      app.usage().size === 0
                        ? "no workflow files were read"
                        : app.nothingSearched()
                          ? "no saved workflow files to search"
                          : "no saved workflow names them, so they can probably be deleted"
                    }
                  />
                </div>

                <Show when={cutOffLine(app.dl.downloads())}>
                  {(line) => (
                    <div class="note up">
                      {line().text}{" "}
                      <button class="lnk" onClick={() => app.actions.go("download")}>
                        {line().link}
                      </button>
                    </div>
                  )}
                </Show>
                <Show when={app.cutOffRun()}>
                  <div class="hero">
                    <div class="txt">
                      <div class="l1">A run stopped part way through.</div>
                      <div class="l2">
                        It can be finished or undone. Nothing else can start until
                        it is settled.
                      </div>
                    </div>
                    <button class="btn" onClick={() => app.actions.go("consolidate")}>
                      Finish it or undo it
                    </button>
                  </div>
                </Show>
                <Show when={run()?.state === "partlyReverted"}>
                  <div class="hero">
                    <div class="txt">
                      <div class="l1">An undo stopped part way.</div>
                      <div class="l2">
                        Some files are back where they were and the rest are still in
                        the vault behind their links. Every model still loads in
                        ComfyUI.
                      </div>
                    </div>
                    <button class="btn" onClick={() => app.actions.go("consolidate")}>
                      See where it stopped
                    </button>
                  </div>
                </Show>
                <Show
                  when={!app.cutOffRun() && run()?.state !== "partlyReverted" && run()}
                  fallback={
                    <Show when={!run()}>
                      <PlanHero afterFree={afterFree()} />
                    </Show>
                  }
                >
                  {(finished) => (
                    <div class="hero done">
                      <div class="big">
                        {fmtN(finished().bytesFreed)}
                        <small>{fmtU(finished().bytesFreed)}</small>
                      </div>
                      <div class="txt">
                        <div class="l1">
                          Freed.{" "}
                          {finished().linksCreated === 1
                            ? "1 copy is now a link."
                            : `${finished().linksCreated} copies are now links.`}
                        </div>
                        <div class="l2">
                          <Show
                            when={drive()?.freeBytes != null}
                            fallback={
                              <>
                                Drive {app.vaultVolume()} did not answer when asked
                                how much room it has.
                              </>
                            }
                          >
                            Drive {app.vaultVolume()} has{" "}
                            {fmt(drive()!.freeBytes!)} free.
                          </Show>
                        </div>
                      </div>
                      <button class="btn" onClick={() => app.actions.go("consolidate")}>
                        Open the report
                      </button>
                    </div>
                  )}
                </Show>
              </>
          </Show>

          <div class="sec secgap">
            <span class="t">Installs</span>
            <Show
              when={!app.nothingRead() && totals()}
              fallback={<span class="n">{app.installs().length} registered</span>}
            >
              <span class="n">
                {fmt(held().bytesOnDisk)} of models across {app.installs().length}{" "}
                installs
              </span>
            </Show>
          </div>
          <For each={app.installViews()}>
            {(view) => (
              <div class="inst">
                <span class="led" classList={{ up: view.running, idle: !view.running }} />
                <span class="name nm" title={view.install.root}>
                  {installName(view.install, app.installs())}
                </span>
                <span class="path pp">{view.install.root}</span>
                <Show when={view.install.extraPaths.length > 0}>
                  <span class="faint yaml">+ extra_model_paths.yaml</span>
                </Show>
                <Show
                  when={!app.nothingRead()}
                  fallback={<span class="faint notread">not read yet</span>}
                >
                  <span class="num c1">{view.files}</span>
                  <span class="num c2">{fmt(view.bytes)}</span>
                </Show>
                <Show when={view.running} fallback={<span class="pill link">idle</span>}>
                  <span
                    class="pill up"
                    title={processesFor(view.install.id, app.running())
                      .map(processTooltip)
                      .join("\n")}
                  >
                    running
                  </span>
                </Show>
              </div>
            )}
          </For>

          <ThumbnailNote />

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
                    <span class="dim">&middot; {countOf(entry.files, "file", "files")}</span>
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

/**
 * Nothing has been read: the only scan was cancelled. Adding an install is
 * offered here as well, so finishing the list never needs another screen.
 */
function NotScannedYet() {
  const app = useApp();
  const n = () => app.installs().length;
  return (
    <div class="hero">
      <div class="txt">
        <div class="l1">
          {n()} {n() === 1 ? "install is" : "installs are"} registered and nothing
          has been read yet.
        </div>
        <div class="l2">
          <Show when={app.scan()?.cancelled}>
            The last scan was cancelled before it finished.{" "}
          </Show>
          A scan reads every model file once to find the identical ones. Nothing
          moves until you say so.
        </div>
      </div>
      <button
        class="btn"
        disabled={!app.hasVault()}
        onClick={() => void openInstallPicker(app)}
      >
        <Icon name="plus" size={13} />
        Add an install
      </button>
      <button
        class="btn pri"
        onClick={() => void app.actions.run(() => app.engine.startScan())}
      >
        <Icon name="scan" size={13} />
        {scanLabel(n())}
      </button>
    </div>
  );
}

function PlanHero(props: { afterFree: number | null }) {
  const app = useApp();
  const totals = () => app.plan()?.totals ?? null;
  const drive = () => app.vault();
  return (
    // With nothing left to consolidate there is no plan to review, and the
    // cards above already say where the models are.
    <Show when={app.plan()?.groups.length ? totals() : null}>
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
                    behind, so nothing on drive {app.vaultVolume()} changes size.
                  </div>
                </>
              }
            >
              <div class="l1">
                {countOf(app.scan()?.totals.duplicateFiles ?? 0, "copy", "copies")} of{" "}
                {countOf(t().groupsFreeingSpace, "model", "models")}{" "}
                {(app.scan()?.totals.duplicateFiles ?? 0) === 1 ? "is" : "are"} held twice or more.
              </div>
              <Show when={props.afterFree !== null}>
                <div class="l2">
                  Consolidating leaves {fmt(props.afterFree!)} free on drive{" "}
                  {app.vaultVolume()}
                  <Show when={drive()?.totalBytes != null}>
                    , {usedPercent(drive()!.totalBytes!, props.afterFree!)}% used
                  </Show>
                  .
                </div>
              </Show>
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
    const t = scan.totals;
    lines.push({
      event: scan.cancelled ? "Scan stopped" : "Scan finished",
      // No space figure here. The scan adds up every extra copy it found; the
      // plan card leaves out the ones that cannot move now and the ones that
      // are a second name for the same bytes. Only the plan's figure can be freed.
      detail: `${countOf(installsTotalsOf(app.contents()).models, "model", "models")} found, ${countOf(t.duplicateFiles, "extra copy", "extra copies")}.`,
      when: scan.finishedAt,
    });
  }
  const run = app.lastApply();
  if (run?.finishedAt) {
    lines.push({
      event: RUN_EVENT[run.state],
      detail: `${countOf(run.filesMoved, "file", "files")} moved into the vault, ${countOf(run.linksCreated, "link", "links")} made, ${fmt(run.bytesFreed)} freed.`,
      when: run.finishedAt,
    });
  }
  for (const install of app.installs()) {
    lines.push({
      event: "Install added",
      detail: `${installName(install, app.installs())}, at ${install.root}`,
      when: install.addedAt,
    });
  }
  const vault = app.vault();
  if (vault) {
    lines.push({
      event: "Vault created",
      detail: `${vault.root}, on drive ${volumeLabel(vault.volume)}`,
      when: vault.createdAt,
    });
  }
  return lines
    .sort((a, b) => new Date(b.when).getTime() - new Date(a.when).getTime())
    .slice(0, 6);
}

/** What a run's state means, as the Recent list says it. */
const RUN_EVENT: Record<ApplyState, string> = {
  running: "Consolidating",
  completed: "Consolidated",
  completedWithErrors: "Consolidated, not all",
  cancelled: "Consolidation stopped",
  interrupted: "Consolidation cut off",
  partlyReverted: "Undo not finished",
  reverted: "Consolidation undone",
  setAside: "Run set aside",
};

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
