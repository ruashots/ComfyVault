import { For, Show, createMemo, type JSX } from "solid-js";

import { Icon, Mark, type IconName } from "~/components/Icon";
import { DanglingLinks } from "~/components/DanglingLinks";
import { ThumbnailNote } from "~/components/ThumbnailNote";
import { Header, Warnbar } from "~/components/Shell";
import { fmt, fmtN, fmtU, relativeTime, usedPercent } from "~/domain/format";
import { driveFor, volumeLabel } from "~/domain/drives";
import { openInstallPicker, openVaultPicker } from "~/modals/picker";
import { useApp } from "~/state/store";
import type { DriveInfo } from "~/ipc/contract";
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
 * Installs lead. Once one is registered the screen can say which drive it is
 * on, which is the fact that makes the vault-folder choice an informed one.
 */
/**
 * The two things a person sets before anything else works, each ticked when it
 * is done, and what happens after they are.
 *
 * The vault comes first because the engine cannot record an install without
 * one, and because a tick that only lives in this window would be a promise
 * the disk is not keeping. Nothing here states a size: a size comes from a
 * scan, and a scan needs the vault that step one is choosing.
 */
function Setup() {
  const app = useApp();
  const vault = () => app.vault();
  const hasInstalls = () => app.hasInstalls();
  const hasVault = () => app.hasVault();
  const vaultDrive = () =>
    vault() ? driveFor(vault()!.root, app.drives()) : null;

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
      ? app.installs().map((i) => i.root).join("  ·  ")
      : hasVault()
        ? "none registered yet"
        : "waiting for the vault folder";

  return (
    <>
      <Header title="Setup" sub={hasVault() ? "1 of 2 done" : "nothing set yet"} />
      <div class="screen">
        <div class="scroll">
          <div style={{ "text-align": "center", padding: "6px 0 0" }}>
            <Mark size={26} />
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

          <StepRow
            number={2}
            title="Your ComfyUI installs"
            sub={installSub()}
            done={hasInstalls()}
          >
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
                <Icon name="folder" size={13} />
                {hasInstalls() ? "Add another" : "Choose an install folder"}
              </button>
            </Show>
          </StepRow>

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
  const run = () => app.lastApply();
  const counted = () => app.planView()?.countedNeverMoved ?? [];

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
                    note={app.installs().map((i) => i.label).join(" · ")}
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
                      app.usage().size > 0 ? String(app.unusedCount()) : "not yet"
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

function PlanHero(props: { afterFree: number | null }) {
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
                    behind, so nothing on drive {app.vaultVolume()} changes size.
                  </div>
                </>
              }
            >
              <div class="l1">
                {app.scan()?.totals.duplicateFiles ?? 0} copies of{" "}
                {t().groupsFreeingSpace} models are held twice or more.
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
    lines.push({
      event: scan.cancelled ? "vault.scan.cancelled" : "vault.scan.complete",
      detail: `${scan.totals.uniqueContents} models · ${scan.totals.duplicateFiles} duplicate copies · ${fmt(scan.totals.reclaimableBytes)} reclaimable`,
      when: scan.finishedAt,
    });
  }
  const run = app.lastApply();
  if (run?.finishedAt) {
    lines.push({
      event: `vault.apply.${run.state}`,
      detail: `${run.filesMoved} files moved · ${run.linksCreated} links · ${fmt(run.bytesFreed)} freed`,
      when: run.finishedAt,
    });
  }
  for (const install of app.installs()) {
    lines.push({
      event: "instance.add",
      detail: `${install.label} · ${install.root}`,
      when: install.addedAt,
    });
  }
  const vault = app.vault();
  if (vault) {
    lines.push({
      event: "vault.create",
      detail: `${vault.root} · drive ${volumeLabel(vault.volume)}`,
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
