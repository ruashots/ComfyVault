import { For, Show, createMemo, type JSX } from "solid-js";

import { Icon } from "~/components/Icon";
import { EmptyScreen, Header } from "~/components/Shell";
import {
  blockedRole,
  blockedShort,
  blockedWhy,
  isFixable,
} from "~/domain/blocked";
import { countOf, fmt, joinPath, relativeTime, shortHash } from "~/domain/format";
import { installName, installNameOf } from "~/domain/installname";
import {
  addedCode,
  copiesOf,
  duplicateAfter,
  duplicateWhy,
  fileNameOf,
  folderOf,
  linkNamesOf,
  modelTitleOf,
  numberWord,
  type ClashView,
} from "~/domain/view";
import { gateBlockers } from "~/domain/selection";
import { ThumbnailNote } from "~/components/ThumbnailNote";
import { useApp } from "~/state/store";
import { ApplyRunning } from "~/screens/Applying";
import { ApplyDone } from "~/screens/ApplyDone";
import { openUndoBox } from "~/modals/undo";
import { RunCutOff } from "~/screens/RunCutOff";
import { UndoStopped } from "~/screens/UndoStopped";
import {
  NO_ANSWER,
  commandText,
  holdsFact,
  labelsOf,
  listeningFact,
  matchText,
  noModelFileKnown,
  onlyPort,
  openTaskManager,
  startedFact,
} from "~/domain/running";
import type { BlockedRow, PlanGroup, RunningComfy } from "~/ipc/contract";

export function ConsolidateScreen() {
  const app = useApp();
  return (
    <Show
      when={app.setupDone()}
      fallback={
        <EmptyScreen
          title="Consolidate"
          head="Nothing to consolidate yet"
          body={`${app.missingStep() ?? ""} ComfyVault then reads your installs and shows exactly what it would move, before it moves anything.`}
        >
          <button class="btn pri" onClick={() => app.actions.go("home")}>
            <Icon name="arrow" size={13} />
            Finish setting up
          </button>
        </EmptyScreen>
      }
    >
      <Show
        when={!app.applyProgress() && !app.revertProgress()}
        fallback={<ApplyRunning />}
      >
        <Show
          when={!app.runOnScreen()}
          fallback={
            <Show
              when={app.cutOffRun()}
              fallback={
                <Show
                  when={app.lastApply()!.state === "partlyReverted"}
                  fallback={<ApplyDone />}
                >
                  <UndoStopped />
                </Show>
              }
            >
              {(run) => <RunCutOff run={run()} />}
            </Show>
          }
        >
          <Show
            when={!app.scanPredatesUndo() && !app.scanPredatesSetAside()}
            fallback={<StaleScan />}
          >
            <Show
              when={app.planView()}
              fallback={
                <EmptyScreen
                  title="Consolidate"
                  head="Nothing has been read yet"
                  body="ComfyVault has to read your installs once before it can say what is duplicated. A scan changes nothing on disk."
                >
                  <button
                    class="btn pri"
                    onClick={() => void app.actions.run(() => app.engine.startScan())}
                  >
                    <Icon name="scan" size={13} />
                    Scan now
                  </button>
                </EmptyScreen>
              }
            >
              <DryRun />
            </Show>
          </Show>
        </Show>
      </Show>
    </Show>
  );
}

/**
 * After an undo or a run set aside, before anything has scanned again.
 *
 * Measured against the real engine: a plan rebuilt from the last scan after an
 * undo reports no model held twice and every restored copy as changed, on a
 * tree that holds every duplicate again. Nobody can act on that plan, so it is
 * not shown. A run set aside had put some models in the vault before it was
 * cut off, so a scan from before it is out of date the same way.
 */
function StaleScan() {
  const app = useApp();
  return (
    <EmptyScreen
      title="Consolidate"
      head="Your installs changed since the last scan"
      body={
        app.scanPredatesSetAside()
          ? "The run you set aside had already put some models in the vault, so the last scan no longer describes your installs. Scan again to see what is duplicated now. A scan changes nothing on disk. The run comes back to be finished or undone once the places it names can be reached again."
          : "The undo put every file back where it was, so the last scan no longer describes your installs. Scan again to see what is duplicated now. A scan changes nothing on disk."
      }
    >
      <button
        class="btn pri"
        onClick={() => void app.actions.run(() => app.engine.startScan())}
      >
        <Icon name="scan" size={13} />
        Scan now
      </button>
    </EmptyScreen>
  );
}

function DryRun() {
  const app = useApp();
  const view = () => app.planView()!;
  const totals = () => view().plan.totals;

  const shownDuplicates = createMemo(() =>
    app.showAllDuplicates() ? view().duplicates : view().duplicates.slice(0, 10),
  );
  const singlesBytes = createMemo(() =>
    view().singles.reduce((sum, g) => sum + g.sizeBytes, 0),
  );
  const volume = () => app.vaultVolume();

  /** Where the vault sits relative to the installs, said plainly. */
  const vaultNote = createMemo(() => {
    const crossVolume = totals().crossVolumeGroups;
    if (crossVolume === 0) {
      const n = app.installs().length;
      const how = n === 1 ? "the install" : n === 2 ? "both installs" : `all ${n} installs`;
      return `on drive ${volume()}, the same drive as ${how}`;
    }
    return `on drive ${volume()}, and ${crossVolume} ${crossVolume === 1 ? "file" : "files"} will be copied across from another drive`;
  });

  return (
    <>
      <Header title="Consolidate" sub="A dry run. Nothing moves until you apply it.">
        <button
          class="btn"
          onClick={() => void app.actions.run(() => app.engine.startScan())}
        >
          <Icon name="refresh" size={13} />
          Run the dry run again
        </button>
      </Header>
      <div class="screen">
        <div class="scroll">
          <Show when={gateBlockers(app.gate()).length > 0}>
            <BlockerPanel />
          </Show>

          {/* A plan made after a finished run replaces that run's screen, and
              the run can still be undone from here. */}
          <Show when={app.lastApply()?.revertible ? app.lastApply() : null}>
            {(last) => (
              <div class="note" style={{ "margin-bottom": "9px" }}>
                This plan comes from a scan taken after your last run, which
                finished {relativeTime(last().finishedAt ?? last().startedAt)}. That
                run can still be undone.{" "}
                <button class="lnk" onClick={() => void openUndoBox(app, last().applyId)}>
                  Undo the last run
                </button>
              </div>
            )}
          </Show>

          <div class="sec">
            <span class="t">What this run will do</span>
            <span class="n">nothing has moved yet</span>
          </div>
          <div class="kv">
            <span class="k w150">Space to be freed</span>
            <span class="v">
              <b>{fmt(totals().bytesFreed)}</b>{" "}
              <span class="dim">
                once{" "}
                {totals().linksCreated === 1
                  ? "1 copy is replaced by a link"
                  : `${totals().linksCreated} copies are replaced by links`}
              </span>
            </span>
          </div>
          <div class="kv">
            <span class="k w150">Vault folder</span>
            <span class="v">
              {view().plan.vaultRoot} <span class="dim">{vaultNote()}</span>
            </span>
          </div>
          <For each={app.installViews()}>
            {(install) => (
              <div class="kv">
                <span class="k w150 install" title={install.install.root}>
                  {installName(install.install, app.installs())}
                </span>
                <span class="v">
                  <Show
                    when={install.moving > 0 || install.stuck > 0}
                    fallback={
                      <span class="dim">
                        no copy in {install.install.root} will be replaced by a link
                      </span>
                    }
                  >
                    <b>{install.moving}</b>
                    <Show
                      when={install.stuck > 0}
                      fallback={
                        install.moving === 1
                          ? " copy will be replaced by a link"
                          : " copies will be replaced by links"
                      }
                    >
                      {" "}
                      of {countOf(install.files, "copy", "copies")}{" "}
                      {install.moving === 1 ? "will be replaced by a link" : "will be replaced by links"}
                    </Show>
                    ,{" "}
                    <span class="dim">
                      and {fmt(install.movingBytes)} will leave {install.install.root}
                    </span>
                    <Show when={install.stuck > 0}>
                      <span class="red">, and {install.stuck} will stay where they are</span>
                    </Show>
                  </Show>
                </span>
              </div>
            )}
          </For>
          <div class="kv">
            <span class="k w150">The vault will hold</span>
            <span class="v">
              <b>{fmt(totals().bytesMoved)}</b>{" "}
              <span class="dim">
                in {totals().filesMoved} {totals().filesMoved === 1 ? "file" : "files"}, one
                copy of each model
              </span>
            </span>
          </div>
          <div class="kv">
            <span class="k w150">Will not move</span>
            <span class="v">
              <Show
                when={totals().blockedRows > 0}
                fallback={
                  <>
                    nothing <span class="dim">ComfyVault can move every file</span>
                  </>
                }
              >
                {fmt(totals().blockedBytes)}{" "}
                <span class="dim">
                  in {totals().blockedRows} {totals().blockedRows === 1 ? "file" : "files"}{" "}
                  ComfyVault cannot move right now
                </span>
              </Show>
            </span>
          </div>
          <Show when={view().countedNeverMoved.length > 0}>
            <div class="kv">
              <span class="k w150">Left alone</span>
              <span class="v">
                {fmt(view().countedNeverMoved.reduce((s, c) => s + c.bytes, 0))}{" "}
                <span class="dim">
                  of weights inside custom_nodes and the Hugging Face cache
                </span>
              </span>
            </div>
          </Show>
          <div class="note up">
            Every file stays reachable at the path it has today, so no workflow and
            no setting in ComfyUI needs changing.
          </div>
          <ThumbnailNote where="plan" />

          <div class="sec secgap">
            <span class="t">Same file, in more than one place</span>
            <span class="n">
              {view().duplicates.length} {view().duplicates.length === 1 ? "model" : "models"},{" "}
              {fmt(totals().bytesFreed)} to be freed
            </span>
          </div>
          <Show
            when={view().duplicates.length > 0}
            fallback={
              <div class="note">
                No model has more than one copy, so this run will free no space.
              </div>
            }
          >
            <div class="note lead">
              Each model below has copies in more than one folder, and every copy has
              the same SHA-256 fingerprint, so they are the same file, byte for byte.
              The vault will get one copy, and{" "}
              <span class="emph">each copy listed will be replaced by a link to it</span>,
              with the same name, in the same folder. ComfyUI will load the model as
              before, and the space of the extra copies will be freed.
            </div>
            <For each={shownDuplicates()}>
              {(group) => <DuplicateGroup group={group} />}
            </For>
            <Show when={view().duplicates.length > 10}>
              <div class="more">
                <Show
                  when={app.showAllDuplicates()}
                  fallback={
                    <button
                      class="btn sm"
                      onClick={() => app.actions.setShowAllDuplicates(true)}
                    >
                      Show all {view().duplicates.length} models
                    </button>
                  }
                >
                  <button
                    class="btn sm"
                    onClick={() => app.actions.setShowAllDuplicates(false)}
                  >
                    Show the 10 biggest only
                  </button>
                </Show>
              </div>
            </Show>
          </Show>

          <div class="sec secgap">
            <span class="t">Different files with the same name</span>
            <span class="n">
              {view().clashes.length} {view().clashes.length === 1 ? "name" : "names"}
            </span>
          </div>
          <Show
            when={view().clashes.length > 0}
            fallback={
              <div class="note">
                No two different files share a name. Every model will go into the
                vault under the name it has now.
              </div>
            }
          >
            <div class="note lead">
              Each name below is used by different models: the files have the same
              name but different content. Every one of them will go into the vault.
              Two files cannot have the same name in the vault, so the second one will
              get a short code added to its name there.{" "}
              <span class="emph">
                Each file listed will be replaced by a link with its current name
              </span>
              , so each ComfyUI install will still load its model under the name it
              uses now.
            </div>
            <For each={view().clashes}>{(clash) => <ClashBlock clash={clash} />}</For>
          </Show>

          <div class="sec secgap">
            <span class="t">One copy only, so nothing will be freed yet</span>
            <span class="n">
              {view().singles.length} {view().singles.length === 1 ? "file" : "files"},{" "}
              {fmt(singlesBytes())}
            </span>
          </div>
          <Show
            when={view().singles.length > 0}
            fallback={
              <div class="note">
                No model has only one copy.
              </div>
            }
          >
            <div class="note lead">
              Each of these exists once, so moving it into the vault will free
              nothing yet. After the run, you can delete the ones you no longer need
              in <span class="emph">Cleanup</span>.
            </div>
            <Show
              when={app.showSingles()}
              fallback={
                <div style={{ padding: "2px 0" }}>
                  <button
                    class="btn sm"
                    onClick={() => app.actions.setShowSingles(true)}
                  >
                    List all {view().singles.length}
                  </button>
                </div>
              }
            >
              <For each={view().singles}>
                {(group) => {
                  const on = () => !app.unticked().has(group.groupId);
                  return (
                    <div class="grp" classList={{ off: !on() }}>
                      <button
                        class="grp-h"
                        aria-pressed={on()}
                        aria-label={`Include ${modelTitleOf(group)}`}
                        onClick={() => app.actions.toggleGroup(group.groupId)}
                      >
                        <Checkbox on={on()} decorative />
                        <span class="grp-n" title={group.source.absPath}>
                          {modelTitleOf(group)}
                        </span>
                        <span
                          class="who wide"
                          style={{ color: "var(--t-muted)", "font-size": "10px" }}
                          title={
                            app.installs().find((i) => i.id === group.source.installId)?.root
                          }
                        >
                          {installNameOf(group.source.installId, app.installs())}
                        </span>
                        <span class="grp-s" style={{ color: "var(--t-body)" }}>
                          {fmt(group.sizeBytes)}
                        </span>
                      </button>
                    </div>
                  );
                }}
              </For>
              <div style={{ padding: "9px 0 2px" }}>
                <button
                  class="btn sm"
                  onClick={() => app.actions.setShowSingles(false)}
                >
                  Collapse
                </button>
              </div>
            </Show>
          </Show>

          <div class="sec secgap">
            <span class="t">Files that cannot move</span>
            <span class="n">
              <Show when={view().blocked.length > 0} fallback="none">
                {view().blocked.length} {view().blocked.length === 1 ? "file" : "files"},{" "}
                {fmt(totals().blockedBytes)}
              </Show>
            </span>
          </div>
          <Show
            when={view().blocked.length > 0}
            fallback={
              <div class="note">
                Every copy can move. Nothing is held open, nothing is in the way,
                and Windows refused nothing.
              </div>
            }
          >
            <div class="note" style={{ margin: "-4px 0 8px" }}>
              Nothing here is skipped quietly. Each one names what stopped it.
            </div>
            <For each={view().blocked.slice(0, 40)}>
              {(row) => <BlockedRowView row={row} />}
            </For>
            <Show when={view().blocked.length > 40}>
              <div class="note up">
                {view().blocked.length - 40} more are in the same state. Fixing the
                reasons above clears them together.
              </div>
            </Show>
          </Show>
        </div>

        <CommitBar />
      </div>
    </>
  );
}

export function Checkbox(props: {
  on: boolean;
  dead?: boolean;
  decorative?: boolean;
  label?: string;
  onToggle?: () => void;
}) {
  if (props.decorative || props.dead) {
    return (
      <span class="cb" classList={{ on: props.on, dead: props.dead }}>
        <Icon name={props.dead ? "x" : "check"} size={9} />
      </span>
    );
  }
  return (
    <button
      class="cb"
      classList={{ on: props.on }}
      role="checkbox"
      aria-checked={props.on}
      aria-label={props.label}
      onClick={(e) => {
        e.stopPropagation();
        props.onToggle?.();
      }}
    >
      <Icon name="check" size={9} />
    </button>
  );
}

function DuplicateGroup(props: { group: PlanGroup }) {
  const app = useApp();
  const on = () => !app.unticked().has(props.group.groupId);
  const title = () => modelTitleOf(props.group);
  const vaultName = () => fileNameOf(props.group.vaultRelPath);
  const manyNames = () => linkNamesOf(props.group).length > 1;
  const why = () => duplicateWhy(props.group);

  return (
    <div class="grp" classList={{ off: !on() }}>
      <button
        class="grp-h"
        aria-pressed={on()}
        aria-label={`Include ${title()}`}
        onClick={() => app.actions.toggleGroup(props.group.groupId)}
      >
        <Checkbox on={on()} decorative />
        <span class="grp-n" title={title()}>
          {title()}
        </span>
        <span class="grp-s">
          {fmt(props.group.bytesFreed)}
          <em>to be freed</em>
        </span>
      </button>
      <div class="grp-sub">
        {copiesOf(props.group, fmt(props.group.sizeBytes))}, SHA-256{" "}
        <span class="h">{shortHash(props.group.sha256)}</span>
      </div>
      <div class="grp-b">
        <For each={props.group.links}>
          {(link) => (
            <div class="cp">
              <CopyPath path={link.absPath} alt={manyNames() && link.nameDiffersFromVault} />
              <Show when={link.sharesBytesWithAnother}>
                <span class="to">frees nothing</span>
              </Show>
            </div>
          )}
        </For>
      </div>
      <div class="grp-after">
        <Icon name="link" size={11} />
        <span class="vp" title={joinPath(app.plan()?.vaultRoot ?? "", props.group.vaultRelPath)}>
          {duplicateAfter(props.group)}{" "}
          <span class="faint">
            {joinPath(app.plan()?.vaultRoot ?? "", folderOf(props.group.vaultRelPath))}
          </span>
          {vaultName()}
        </span>
      </div>
      <Show when={why().length > 0}>
        <div class="grp-why">
          <For each={why()}>
            {(part, i) => (
              <>
                {i() > 0 ? " " : ""}
                <Show when={part.alt} fallback={part.text}>
                  <span class="alt">{part.text}</span>
                </Show>
              </>
            )}
          </For>
        </div>
      </Show>
    </div>
  );
}

/**
 * A copy's full path. The folder gives way first, so the file name, which is
 * what tells two copies apart, stays readable longest.
 */
function CopyPath(props: { path: string; alt?: boolean }) {
  return (
    <span class="pp split" title={props.path}>
      <span class="pd">{folderOf(props.path)}</span>
      <span class="pf" classList={{ alt: props.alt }}>
        {fileNameOf(props.path)}
      </span>
    </span>
  );
}

/**
 * One name that different models want. Each model says the name it gets in
 * the vault, then lists the copies that will link to it.
 */
function ClashBlock(props: { clash: ClashView }) {
  const app = useApp();
  return (
    <div class="grp cgrp">
      <div class="grp-h static">
        <span class="grp-n">{props.clash.filename}</span>
        <span class="grp-s clash-n">
          {numberWord(props.clash.groups.length)} different models
        </span>
      </div>
      <For each={props.clash.groups}>
        {(group) => {
          const on = () => !app.unticked().has(group.groupId);
          const vaultName = () => fileNameOf(group.vaultRelPath);
          const code = () => addedCode(vaultName(), props.clash.filename);
          return (
            <div class="cm" classList={{ off: !on() }}>
              <button
                class="grp-h"
                aria-pressed={on()}
                aria-label={`Include the ${fmt(group.sizeBytes)} ${props.clash.filename}`}
                onClick={() => app.actions.toggleGroup(group.groupId)}
              >
                <Checkbox on={on()} decorative />
                <span
                  class="cm-to"
                  title={joinPath(app.plan()?.vaultRoot ?? "", group.vaultRelPath)}
                >
                  into the vault as{" "}
                  <span class="vn">
                    <Show when={code()} fallback={vaultName()}>
                      {(c) => (
                        <>
                          {c().before}
                          <span class="tag">{c().code}</span>
                          {c().after}
                        </>
                      )}
                    </Show>
                  </span>
                </span>
                <span class="grp-s">{fmt(group.sizeBytes)}</span>
              </button>
              <div class="grp-b">
                <For each={group.links}>
                  {(link) => (
                    <div class="cp">
                      <CopyPath path={link.absPath} />
                    </div>
                  )}
                </For>
              </div>
            </div>
          );
        }}
      </For>
    </div>
  );
}

function BlockedRowView(props: { row: BlockedRow }) {
  const app = useApp();

  const recheck = async () => {
    await app.actions.refresh();
    const still = app
      .planView()
      ?.blocked.some((b) => b.absPath === props.row.absPath);
    app.actions.showToast(
      still
        ? `Checked · ${blockedShort(props.row.reason)}, still`
        : "Checked · that one can move now",
      still ? "bad" : "ok",
    );
  };

  return (
    <div class="grp dead">
      <div class="grp-h static">
        <Checkbox on={false} dead />
        <span class="grp-n">{fileNameOf(props.row.absPath)}</span>
        <span class="grp-s">
          {fmt(props.row.sizeBytes)}
          <em>stays</em>
        </span>
      </div>
      <div class="grp-b">
        <div class="cp">
          <span class="role stay">{blockedRole(props.row.reason)}</span>
          <span
            class="who"
            title={app.installs().find((i) => i.id === props.row.installId)?.root}
          >
            {props.row.installId
              ? installNameOf(props.row.installId, app.installs())
              : "outside an install"}
          </span>
          <span class="pp">{props.row.absPath}</span>
        </div>
      </div>
      <div class="grp-why wide">{blockedWhy(props.row, app.installs())}</div>
      <Show when={isFixable(props.row.reason)}>
        <div class="grp-fix">
          <button class="btn sm" onClick={() => void recheck()}>
            Re-check
          </button>
        </div>
      </Show>
    </div>
  );
}

function BlockerPanel() {
  const app = useApp();
  const blockers = () => gateBlockers(app.gate());

  const recheck = async () => {
    await app.actions.refresh();
    const left = gateBlockers(app.gate());
    app.actions.showToast(
      left.length === 0
        ? `Checked · nothing is in the way, ${fmt(app.plan()?.totals.bytesFreed ?? 0)} can be freed`
        : `Checked · ${left.length} ${left.length === 1 ? "thing is" : "things are"} still in the way`,
      left.length === 0 ? "ok" : "bad",
    );
  };

  return (
    <div class="blk">
      <h3>
        <Icon name="warn" size={13} />
        Apply is blocked &middot; {blockers().length}{" "}
        {blockers().length === 1 ? "thing" : "things"} to fix
      </h3>
      <For each={blockers()}>
        {(blocker) => (
          <Show
            when={blocker.kind === "symlinks_unsupported" ? blocker : null}
            fallback={
              <Show when={blocker.kind === "comfy_running" ? blocker : null}>
                {(comfy) => (
                  <For each={comfy().processes}>
                    {(p) => <RunningRow process={p} />}
                  </For>
                )}
              </Show>
            }
          >
            {(links) => (
              <div class="blkrow">
                <div class="bl">
                  <div class="bt">Windows will not let this app create links</div>
                  <div class="bd">
                    {links().guidance ??
                      "A link is what keeps ComfyUI working after a file moves, and this system will not make one right now."}
                  </div>
                  <Show when={links().probeError}>
                    {(why) => <div class="where">{why()}</div>}
                  </Show>
                </div>
                <div class="ba">
                  <button
                    class="btn sm"
                    onClick={() =>
                      void app.engine.openExternal("ms-settings:developers")
                    }
                  >
                    <Icon name="external" size={11} />
                    Open that page
                  </button>
                  <button class="btn sm pri" onClick={() => void recheck()}>
                    <Icon name="refresh" size={11} />
                    Check again
                  </button>
                </div>
              </div>
            )}
          </Show>
        )}
      </For>
      <div class="scope">
        Everything else works, and the report below is complete. Only Apply is held
        back.
      </div>
    </div>
  );
}

/**
 * One running ComfyUI, told in facts Windows gave and the person can find
 * again in Task Manager. ComfyVault never ends it: the buttons open the page it
 * serves, so the person can see it is real, and Task Manager, where they can
 * end it themselves.
 */
function RunningRow(props: { process: RunningComfy }) {
  const app = useApp();
  const p = () => props.process;
  const roots = () =>
    p()
      .matchedInstallIds.map((id) => app.installs().find((i) => i.id === id)?.root ?? id)
      .join(", ");

  const recheck = async () => {
    await app.actions.refresh();
    const still = app.running().find((r) => r.pid === p().pid);
    if (still) {
      app.actions.showToast(
        `Checked · ${labelsOf(still, app.installs())} is still running, pid ${still.pid}`,
        "bad",
      );
      return;
    }
    const left = gateBlockers(app.gate());
    app.actions.showToast(
      left.length === 0
        ? `Checked · nothing is in the way, ${fmt(app.plan()?.totals.bytesFreed ?? 0)} can be freed`
        : `Checked · ${left.length} ${left.length === 1 ? "thing is" : "things are"} still in the way`,
      left.length === 0 ? "ok" : "bad",
    );
  };

  return (
    <div class="blkrow">
      <div class="bl">
        <div class="bt">{labelsOf(p(), app.installs())} is running</div>
        <div class="bd">
          Windows will not move a file while a program has it open. Closing it lets
          every file it is holding move too.
        </div>
        <div class="proc">
          <Fact label="Process">
            {p().name} <span class="dim">&middot;</span> pid {p().pid}
          </Fact>
          <Fact label="Started">
            <Show when={startedFact(p())} fallback={<Unknown />}>
              {(s) => (
                <>
                  {s().when} <span class="dim">&middot; {s().ago}</span>
                </>
              )}
            </Show>
          </Fact>
          <Fact label="Listening on">
            <Show when={listeningFact(p())} fallback={<Unknown />}>
              {(l) => (
                <>
                  {l().value} <span class="dim">&middot; {l().note}</span>
                </>
              )}
            </Show>
          </Fact>
          <Fact label="Model files">
            <Show
              when={holdsFact(
                p(),
                noModelFileKnown(app.scan(), app.vault()?.fileCount ?? null),
              )}
              fallback={<Unknown />}
            >
              {(h) => (
                <>
                  {h().value}
                  <Show when={h().note}>
                    {(note) => <span class="dim"> &middot; {note()}</span>}
                  </Show>
                </>
              )}
            </Show>
          </Fact>
          <Show when={p().exePath}>
            {(exe) => <Fact label="Program">{exe()}</Fact>}
          </Show>
          <Show when={commandText(p())}>
            {(cmd) => <Fact label="Command">{cmd()}</Fact>}
          </Show>
          <Fact label="Why this install">{matchText(p(), roots())}</Fact>
        </div>
        <div class="how">
          Closing the browser tab leaves ComfyUI running. Stop it where you started
          it, or open Task Manager, go to the Details tab and end{" "}
          <span class="emph">pid {p().pid}</span>. ComfyVault does not stop programs
          itself.
        </div>
      </div>
      <div class="ba stack">
        <Show when={onlyPort(p())}>
          {(port) => (
            <button
              class="btn sm"
              onClick={() => void app.engine.openExternal(`http://127.0.0.1:${port()}/`)}
            >
              <Icon name="external" size={11} />
              Open 127.0.0.1:{port()}
            </button>
          )}
        </Show>
        <button class="btn sm" onClick={() => void openTaskManager(app)}>
          <Icon name="external" size={11} />
          Open Task Manager
        </button>
        <button class="btn sm pri" onClick={() => void recheck()}>
          <Icon name="refresh" size={11} />
          Check again
        </button>
      </div>
    </div>
  );
}

function Fact(props: { label: string; children: JSX.Element }) {
  return (
    <div class="pr">
      <span class="pk">{props.label}</span>
      <span class="pv">{props.children}</span>
    </div>
  );
}

function Unknown() {
  return <span class="dim">{NO_ANSWER}</span>;
}

function CommitBar() {
  const app = useApp();
  const selection = () => app.selection();
  const gate = () => app.gate();
  const volume = () => app.vaultVolume();

  const apply = () => {
    const planId = app.plan()?.planId;
    if (!planId) return;
    // An empty run is not a run: it would report zeros as though it had done
    // them. The gate already knows, and this reads the gate rather than asking
    // the same question a second way, so the two can never drift apart.
    const answer = gate();
    if (!answer.can) {
      app.actions.showToast(
        answer.reason === "nothing_ticked"
          ? "Nothing is ticked, so there is nothing to apply"
          : "Apply is held back until the things above are fixed",
        "bad",
      );
      return;
    }
    void app.actions.run(() =>
      app.engine.startApply({
        planId,
        groupIds: [...selection().groupIds],
        verify: "sizeAndMtime",
        stopOnError: false,
      }),
    );
  };

  return (
    <div class="commit">
      <span class="n say">
        <b>{selection().links}</b> {selection().links === 1 ? "copy" : "copies"} will be
        replaced by {selection().links === 1 ? "a link" : "links"} to{" "}
        <b>{selection().moves}</b> vault {selection().moves === 1 ? "file" : "files"}
      </span>
      <span class="g">{fmt(selection().bytes)}</span>
      <span class="n dim">
        to be freed<span class="drv"> on drive {volume()}</span>
      </span>
      <Show
        when={gate().can}
        fallback={
          <Show
            when={gateBlockers(gate()).length}
            fallback={
              <button class="btn" disabled>
                Nothing is ticked
              </button>
            }
          >
            {(count) => (
              <button class="btn" disabled>
                <Icon name="warn" size={13} />
                Apply blocked &middot; {count()} to fix
              </button>
            )}
          </Show>
        }
      >
        <button class="btn pri" onClick={apply}>
          <Icon name="arrow" size={13} />
          Apply this plan
        </button>
      </Show>
    </div>
  );
}
