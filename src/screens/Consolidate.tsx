import { For, Show, createMemo } from "solid-js";

import { Icon } from "~/components/Icon";
import { EmptyScreen, Header } from "~/components/Shell";
import {
  blockedRole,
  blockedShort,
  blockedWhy,
  isFixable,
} from "~/domain/blocked";
import { fmt } from "~/domain/format";
import { chosenBecauseText, fileNameOf } from "~/domain/view";
import { gateBlockers } from "~/domain/selection";
import { ThumbnailNote } from "~/components/ThumbnailNote";
import { useApp } from "~/state/store";
import { ApplyRunning } from "~/screens/Applying";
import { ApplyDone } from "~/screens/ApplyDone";
import { RunCutOff } from "~/screens/RunCutOff";
import { UndoStopped } from "~/screens/UndoStopped";
import type { BlockedRow, PlanGroup } from "~/ipc/contract";

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
          when={!app.lastApply()}
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
          <Show when={!app.scanPredatesUndo()} fallback={<StaleAfterUndo />}>
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
 * After an undo, before anything has scanned again.
 *
 * Measured against the real engine: a plan rebuilt from the last scan after an
 * undo reports no model held twice and every restored copy as changed, on a
 * tree that holds every duplicate again. Nobody can act on that plan, so it is
 * not shown.
 */
function StaleAfterUndo() {
  const app = useApp();
  return (
    <EmptyScreen
      title="Consolidate"
      head="Your installs changed since the last scan"
      body="The undo put every file back where it was, so the last scan no longer describes your installs. Scan again to see what is duplicated now. A scan changes nothing on disk."
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
      return `drive ${volume()}, same drive as ${how}`;
    }
    return `drive ${volume()} · ${crossVolume} ${crossVolume === 1 ? "file comes" : "files come"} from another drive`;
  });

  return (
    <>
      <Header title="Consolidate" sub="dry run · read it before anything moves">
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

          <div class="sec">
            <span class="t">The plan</span>
            <span class="n">dry run &middot; nothing has moved</span>
          </div>
          <div class="kv">
            <span class="k">Space returned</span>
            <span class="v">
              <b>{fmt(totals().bytesFreed)}</b>{" "}
              <span class="dim">
                once {totals().linksCreated} copies become links
              </span>
            </span>
          </div>
          <div class="kv">
            <span class="k">Vault folder</span>
            <span class="v">
              {view().plan.vaultRoot} <span class="dim">{vaultNote()}</span>
            </span>
          </div>
          <For each={app.installViews()}>
            {(install) => (
              <div class="kv">
                <span class="k">{install.install.label}</span>
                <span class="v">
                  <b>{install.moving}</b> of {install.files} files become links{" "}
                  <span class="dim">
                    &middot; {fmt(install.movingBytes)} leaves the folder
                  </span>
                  <Show when={install.stuck > 0}>
                    <span class="red"> &middot; {install.stuck} stay put</span>
                  </Show>
                </span>
              </div>
            )}
          </For>
          <div class="kv">
            <span class="k">The vault holds</span>
            <span class="v">
              <b>{fmt(totals().bytesMoved)}</b>{" "}
              <span class="dim">
                &middot; {totals().filesMoved} files, one copy of each
              </span>
            </span>
          </div>
          <div class="kv">
            <span class="k">Stays where it is</span>
            <span class="v">
              {fmt(totals().blockedBytes)}{" "}
              <span class="dim">
                &middot; {totals().blockedRows} files ComfyVault cannot move right
                now
              </span>
            </span>
          </div>
          <Show when={view().countedNeverMoved.length > 0}>
            <div class="kv">
              <span class="k">Never touched</span>
              <span class="v">
                {fmt(view().countedNeverMoved.reduce((s, c) => s + c.bytes, 0))}{" "}
                <span class="dim">
                  &middot; weights inside custom_nodes and the Hugging Face cache
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
            <span class="t">Duplicates: this is your easy win</span>
            <span class="n">
              {view().duplicates.length} models &middot; {fmt(totals().bytesFreed)}
            </span>
          </div>
          <Show
            when={view().duplicates.length > 0}
            fallback={
              <div class="note">
                No model is held twice. Every file already exists once, so this run
                returns nothing to the drive.
              </div>
            }
          >
            <div class="note" style={{ margin: "-4px 0 8px" }}>
              One copy moves into the vault. Every other copy becomes a link
              pointing at it. ComfyUI reads a link exactly as it reads the file,
              from the same path it used before.
            </div>
            <For each={shownDuplicates()}>
              {(group) => <DuplicateGroup group={group} />}
            </For>
            <Show when={view().duplicates.length > 10}>
              <div style={{ padding: "9px 0 2px" }}>
                <Show
                  when={app.showAllDuplicates()}
                  fallback={
                    <button
                      class="btn sm"
                      onClick={() => app.actions.setShowAllDuplicates(true)}
                    >
                      Show all {view().duplicates.length} groups
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
            <span class="t">Same name, different file</span>
            <span class="n">{view().clashes.length} names</span>
          </div>
          <Show
            when={view().clashes.length > 0}
            fallback={
              <div class="note">
                No two different files share a filename. Nothing has to be renamed
                inside the vault.
              </div>
            }
          >
            <div class="note" style={{ margin: "-4px 0 8px" }}>
              These files share a filename but hold different bytes. Every one is
              kept. All but the first get part of their fingerprint added to the
              name inside the vault, so nothing is lost, and each install keeps the
              name it already uses.
            </div>
            <For each={view().clashes}>
              {(clash) => (
                <div class="grp">
                  <div class="grp-h static">
                    <span style={{ width: "13px" }} />
                    <span class="grp-n">{clash.filename}</span>
                    <span class="grp-s">{clash.groups.length} different files</span>
                  </div>
                  <div class="grp-b tight">
                    <For each={clash.groups}>
                      {(group) => (
                        <div class="cp mid">
                          <Checkbox
                            on={!app.unticked().has(group.groupId)}
                            label={`Include ${fileNameOf(group.source.relPath)} from ${group.source.installLabel}`}
                            onToggle={() => app.actions.toggleGroup(group.groupId)}
                          />
                          <span class="who wide">{group.source.installLabel}</span>
                          <span class="pp">{group.source.relPath}</span>
                          <span class="num" style={{ width: "62px" }}>
                            {fmt(group.sizeBytes)}
                          </span>
                          <span class="to vault">
                            &rarr; vault\
                            <span
                              classList={{
                                amb: group.vaultNameAdjusted,
                                faint: !group.vaultNameAdjusted,
                              }}
                            >
                              {group.vaultRelPath}
                            </span>
                          </span>
                        </div>
                      )}
                    </For>
                  </div>
                </div>
              )}
            </For>
          </Show>

          <div class="sec secgap">
            <span class="t">Moves, but frees nothing</span>
            <span class="n">
              {view().singles.length} files &middot; {fmt(singlesBytes())}
            </span>
          </div>
          <Show
            when={view().singles.length > 0}
            fallback={
              <div class="note">
                Every model here exists more than once, so nothing falls into this
                group.
              </div>
            }
          >
            <div class="note" style={{ margin: "-4px 0 8px" }}>
              Each of these exists once. It still goes into the vault, so every
              model lives in one place and each install reads it through a link.
              The drive gains nothing from these, and nothing is lost either.
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
                        aria-label={`Include ${fileNameOf(group.vaultRelPath)}`}
                        onClick={() => app.actions.toggleGroup(group.groupId)}
                      >
                        <Checkbox on={on()} decorative />
                        <span class="grp-n">{fileNameOf(group.vaultRelPath)}</span>
                        <span
                          class="who wide"
                          style={{ color: "var(--t-muted)", "font-size": "10px" }}
                        >
                          {group.source.installLabel}
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
            <span class="t">Cannot move</span>
            <span class="n">
              {view().blocked.length} files &middot; {fmt(totals().blockedBytes)}
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
  const name = () => fileNameOf(props.group.vaultRelPath);
  const volume = () => app.vaultVolume();

  return (
    <div class="grp" classList={{ off: !on() }}>
      <button
        class="grp-h"
        aria-pressed={on()}
        aria-label={`Include ${name()}`}
        onClick={() => app.actions.toggleGroup(props.group.groupId)}
      >
        <Checkbox on={on()} decorative />
        <span class="grp-n" title={name()}>
          {name()}
        </span>
        <span class="grp-s">
          {fmt(props.group.bytesFreed)}
          <em>back</em>
        </span>
      </button>
      <div class="grp-b">
        <For each={props.group.links}>
          {(link) => {
            const isSource = () => link.absPath === props.group.source.absPath;
            return (
              <div class="cp">
                <span class="role" classList={{ keep: isSource(), link: !isSource() }}>
                  {isSource() ? "keep" : "link"}
                </span>
                <span class="who">{link.installLabel}</span>
                <span class="pp">
                  {link.relPath}
                  <Show when={link.nameDiffersFromVault}>
                    {" "}
                    <span class="amb">{link.linkName}</span>
                  </Show>
                </span>
                <span class="to" classList={{ none: link.sharesBytesWithAnother }}>
                  {isSource()
                    ? props.group.vaultNameAdjusted
                      ? `→ vault\\${props.group.vaultRelPath}`
                      : `→ vault\\${props.group.category}\\`
                    : link.sharesBytesWithAnother
                      ? "frees nothing"
                      : `${fmt(props.group.sizeBytes)} back`}
                </span>
              </div>
            );
          }}
        </For>
      </div>
      <div class="grp-why">
        {chosenBecauseText(props.group, volume())}
        <Show when={props.group.occurrences > props.group.distinctFiles}>
          {" "}
          &middot; {sharedNames(props.group)}
        </Show>
        <Show when={props.group.crossVolume}>
          {" "}
          &middot; one copy is on another drive, so it is copied across and checked
          before the original goes
        </Show>
      </div>
    </div>
  );
}

/**
 * Why the figure counts fewer files than there are paths. Windows lets two
 * names point at one set of bytes, and removing one of them returns nothing
 * while the other name remains. Without this the total quietly disagrees with
 * the paths listed right above it.
 */
function sharedNames(group: PlanGroup): string {
  const extra = group.occurrences - group.distinctFiles;
  return extra === 1
    ? "one of these paths is a second name for a file already listed, so removing it returns no space"
    : `${extra} of these paths are extra names for files already listed, so removing them returns no space`;
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
          <span class="who">{props.row.installLabel ?? "outside an install"}</span>
          <span class="pp">{props.row.absPath}</span>
        </div>
      </div>
      <div class="grp-why wide">{blockedWhy(props.row)}</div>
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
        ? `Checked · nothing is in the way, ${fmt(app.plan()?.totals.bytesFreed ?? 0)} can come back`
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
                  <div class="blkrow">
                    <div class="bl">
                      <div class="bt">
                        {comfy()
                          .processes.map(
                            (p) =>
                              `ComfyUI-${p.matchedInstallIds.map(labelOf(app)).join(", ")}`,
                          )
                          .join(" and ")}{" "}
                        {comfy().processes.length === 1 ? "is" : "are"} running
                      </div>
                      <div class="bd">
                        {comfy()
                          .processes.map((p) => `${p.name}, pid ${p.pid}`)
                          .join("; ")}
                        . Windows will not move a file while a program has it open.
                        Closing it lets every file it is holding move too.
                      </div>
                    </div>
                    <div class="ba">
                      <button class="btn sm pri" onClick={() => void recheck()}>
                        <Icon name="refresh" size={11} />
                        Check again
                      </button>
                    </div>
                  </div>
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

function labelOf(app: ReturnType<typeof useApp>) {
  return (id: string) => app.installs().find((i) => i.id === id)?.label ?? id;
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
      <span class="n">
        <b>{selection().moves}</b> files move
      </span>
      <span class="n dim">&middot;</span>
      <span class="n">
        <b>{selection().links}</b> links go back where they were
      </span>
      <span class="sp" />
      <span class="g">{fmt(selection().bytes)}</span>
      <span class="n dim">back on drive {volume()}</span>
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
