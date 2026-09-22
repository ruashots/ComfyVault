import { For, Show, createMemo } from "solid-js";

import { Icon } from "~/components/Icon";
import { EmptyScreen, Header } from "~/components/Shell";
import { blockedRole, blockedShort, blockedWhy } from "~/domain/blocked";
import { driveOf, fmt } from "~/domain/format";
import type { BlockedPlacement, PlannedModel } from "~/domain/plan";
import { gateBlockers } from "~/domain/selection";
import { openInstancePicker } from "~/modals/picker";
import { useApp } from "~/state/store";
import { ApplyRunning } from "~/screens/Applying";
import { ApplyDone } from "~/screens/ApplyDone";

export function ConsolidateScreen() {
  const app = useApp();
  return (
    <Show
      when={app.hasInstances()}
      fallback={
        <EmptyScreen
          title="Consolidate"
          head="Nothing to consolidate yet"
          body="Register at least one ComfyUI install. ComfyVault then reads it and shows exactly what it would move, before it moves anything."
        >
          <button class="btn pri" onClick={() => void openInstancePicker(app)}>
            <Icon name="folder" size={13} />
            Choose an install folder
          </button>
        </EmptyScreen>
      }
    >
      <Show when={!app.applyProgress()} fallback={<ApplyRunning />}>
        <Show when={!app.lastRun()} fallback={<ApplyDone />}>
          <DryRun />
        </Show>
      </Show>
    </Show>
  );
}

function DryRun() {
  const app = useApp();
  const plan = () => app.plan()!;
  const machine = () => app.machine()!;
  const gate = () => app.gate();

  const shownDuplicates = createMemo(() =>
    app.showAllDuplicates() ? plan().duplicates : plan().duplicates.slice(0, 10),
  );

  const singlesBytes = createMemo(() =>
    plan().singles.reduce((sum, m) => sum + m.bytes, 0),
  );

  /** Where the vault sits relative to the installs, said plainly. */
  const vaultNote = createMemo(() => {
    const letter = driveOf(machine().vaultPath);
    const elsewhere = app
      .scan()!
      .instances.filter((i) => driveOf(i.path) !== letter);
    if (elsewhere.length === 0) {
      const n = app.scan()!.instances.length;
      const how = n === 1 ? "the install" : n === 2 ? "both installs" : `all ${n} installs`;
      return `drive ${letter}, same drive as ${how}`;
    }
    return `drive ${letter} · ${elsewhere.map((i) => i.name).join(" and ")} sit${elsewhere.length === 1 ? "s" : ""} on another drive`;
  });

  return (
    <>
      <Header title="Consolidate" sub="dry run · read it before anything moves">
        <button class="btn" onClick={() => void app.engine.startScan()}>
          <Icon name="refresh" size={13} />
          Run the dry run again
        </button>
      </Header>
      <div class="screen">
        <div class="scroll">
          <Show when={gateBlockers(gate()).length > 0}>
            <BlockerPanel />
          </Show>

          <div class="sec">
            <span class="t">The plan</span>
            <span class="n">dry run &middot; nothing has moved</span>
          </div>
          <div class="kv">
            <span class="k">Space returned</span>
            <span class="v">
              <b>{fmt(plan().totals.reclaimBytes)}</b>{" "}
              <span class="dim">
                once {plan().totals.duplicateCopies} copies become links
              </span>
            </span>
          </div>
          <div class="kv">
            <span class="k">Vault folder</span>
            <span class="v">
              {machine().vaultPath} <span class="dim">{vaultNote()}</span>
            </span>
          </div>
          <For each={app.scan()!.instances}>
            {(instance) => {
              const totals = () => plan().totals.perInstance.get(instance.id);
              return (
                <div class="kv">
                  <span class="k">{instance.name}</span>
                  <span class="v">
                    <b>{totals()?.moving ?? 0}</b> of {totals()?.files ?? 0} files
                    become links{" "}
                    <span class="dim">
                      &middot; {fmt(totals()?.movingBytes ?? 0)} leaves the folder
                    </span>
                    <Show when={totals()?.stuck}>
                      <span class="red"> &middot; {totals()?.stuck} stay put</span>
                    </Show>
                  </span>
                </div>
              );
            }}
          </For>
          <div class="kv">
            <span class="k">The vault holds</span>
            <span class="v">
              <b>
                {fmt(plan().totals.uniqueBytes - plan().totals.vaultOnlyBytes)}
              </b>{" "}
              <span class="dim">
                &middot; {plan().models.length - plan().orphans.length} files, one
                copy of each
              </span>
            </span>
          </div>
          <div class="kv">
            <span class="k">Stays where it is</span>
            <span class="v">
              {fmt(plan().totals.blockedBytes)}{" "}
              <span class="dim">
                &middot; {plan().blocked.length} files ComfyVault cannot move right
                now
              </span>
            </span>
          </div>
          <div class="kv">
            <span class="k">Never touched</span>
            <span class="v">
              {fmt(plan().totals.countedNeverMovedBytes)}{" "}
              <span class="dim">
                &middot; weights inside custom_nodes and the Hugging Face cache
              </span>
            </span>
          </div>
          <div class="note up">
            Every file stays reachable at the path it has today, so no workflow and
            no setting in ComfyUI needs changing.
          </div>

          <div class="sec secgap">
            <span class="t">Duplicates: this is your easy win</span>
            <span class="n">
              {plan().duplicates.length} models &middot;{" "}
              {fmt(plan().totals.reclaimBytes)}
            </span>
          </div>
          <Show
            when={plan().duplicates.length > 0}
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
              {(model) => <DuplicateGroup model={model} />}
            </For>
            <Show when={plan().duplicates.length > 10}>
              <div style={{ padding: "9px 0 2px" }}>
                <Show
                  when={app.showAllDuplicates()}
                  fallback={
                    <button
                      class="btn sm"
                      onClick={() => app.actions.setShowAllDuplicates(true)}
                    >
                      Show all {plan().duplicates.length} groups
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
            <span class="n">{plan().clashes.length} names</span>
          </div>
          <Show
            when={plan().clashes.length > 0}
            fallback={
              <div class="note">
                No two different files share a filename. Nothing has to be renamed
                inside the vault.
              </div>
            }
          >
            <div class="note" style={{ margin: "-4px 0 8px" }}>
              These files share a filename but hold different bytes. Every one is
              kept. All but the largest get a new name inside the vault so nothing
              is lost, and each install keeps the name it already uses.
            </div>
            <For each={plan().clashes}>
              {(group) => (
                <div class="grp">
                  <div class="grp-h static">
                    <span style={{ width: "13px" }} />
                    <span class="grp-n">{group.filename}</span>
                    <span class="grp-s">{group.models.length} different files</span>
                  </div>
                  <div class="grp-b tight">
                    <For each={group.models}>
                      {(model, i) => {
                        const first = () => model.model.placements[0];
                        const where = () => instanceName(app, first()?.instanceId);
                        return (
                          <div class="cp mid">
                            <Checkbox
                              on={!app.unticked().has(model.id)}
                              label={`Include ${model.filename} from ${where()}`}
                              onToggle={() => app.actions.toggleModel(model.id)}
                            />
                            <span class="who wide">{where()}</span>
                            <span class="pp">{first()?.folder}</span>
                            <span class="num" style={{ width: "62px" }}>
                              {fmt(model.bytes)}
                            </span>
                            <span class="to vault">
                              &rarr; vault\{model.folder}\
                              <span classList={{ amb: i() > 0, faint: i() === 0 }}>
                                {model.vaultName}
                              </span>
                            </span>
                          </div>
                        );
                      }}
                    </For>
                  </div>
                </div>
              )}
            </For>
          </Show>

          <div class="sec secgap">
            <span class="t">Moves, but frees nothing</span>
            <span class="n">
              {plan().singles.length} files &middot; {fmt(singlesBytes())}
            </span>
          </div>
          <Show
            when={plan().singles.length > 0}
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
                    List all {plan().singles.length}
                  </button>
                </div>
              }
            >
              <For each={plan().singles}>
                {(model) => {
                  const on = () => !app.unticked().has(model.id);
                  const where = () => model.keeper ?? model.model.placements[0];
                  return (
                    <div class="grp" classList={{ off: !on() }}>
                      <button
                        class="grp-h"
                        aria-pressed={on()}
                        onClick={() => app.actions.toggleModel(model.id)}
                      >
                        <Checkbox on={on()} decorative />
                        <span class="grp-n">{model.filename}</span>
                        <span class="who wide" style={{ color: "var(--t-muted)", "font-size": "10px" }}>
                          {instanceName(app, where()?.instanceId)}
                        </span>
                        <span class="grp-s" style={{ color: "var(--t-body)" }}>
                          {fmt(model.bytes)}
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
              {plan().blocked.length} files &middot;{" "}
              {fmt(plan().totals.blockedBytes)}
            </span>
          </div>
          <Show
            when={plan().blocked.length > 0}
            fallback={
              <div class="note">
                Every copy can move. Nothing is held open, nothing sits on another
                drive, and Windows refused nothing.
              </div>
            }
          >
            <div class="note" style={{ margin: "-4px 0 8px" }}>
              Nothing here is skipped quietly. Each one names what stopped it.
            </div>
            <For each={plan().blocked}>
              {(entry) => <BlockedRow entry={entry} />}
            </For>
          </Show>
        </div>

        <CommitBar />
      </div>
    </>
  );
}

function instanceName(
  app: ReturnType<typeof useApp>,
  id: string | undefined,
): string {
  if (!id) return "";
  return app.scan()?.instances.find((i) => i.id === id)?.name ?? id;
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

function DuplicateGroup(props: { model: PlannedModel }) {
  const app = useApp();
  const on = () => !app.unticked().has(props.model.id);
  const blockedCount = () => props.model.blocked.length;

  return (
    <div class="grp" classList={{ off: !on() }}>
      <button
        class="grp-h"
        aria-pressed={on()}
        aria-label={`Include ${props.model.filename}`}
        onClick={() => app.actions.toggleModel(props.model.id)}
      >
        <Checkbox on={on()} decorative />
        <span class="grp-n" title={props.model.filename}>
          {props.model.filename}
        </span>
        <span class="grp-s">
          {fmt(props.model.reclaimBytes)}
          <em>back</em>
        </span>
      </button>
      <div class="grp-b">
        <For each={props.model.model.placements}>
          {(placement) => {
            const name = () => instanceName(app, placement.instanceId);
            if (placement === props.model.keeper) {
              return (
                <div class="cp">
                  <span class="role keep">keep</span>
                  <span class="who">{name()}</span>
                  <span class="pp">{placement.folder}</span>
                  <span class="to">
                    &rarr; vault\{props.model.folder}\
                  </span>
                </div>
              );
            }
            if (placement.blocked) {
              return (
                <div class="cp">
                  <span class="role stay">stay</span>
                  <span class="who">{name()}</span>
                  <span class="pp">{placement.folder}</span>
                  <span class="to red">{blockedShort(placement.blocked)}</span>
                </div>
              );
            }
            return (
              <div class="cp">
                <span class="role link">link</span>
                <span class="who">{name()}</span>
                <span class="pp">
                  {placement.folder}
                  <Show when={placement.filename !== props.model.filename}>
                    {" "}
                    <span class="amb">{placement.filename}</span>
                  </Show>
                </span>
                <span class="to">{fmt(props.model.bytes)} back</span>
              </div>
            );
          }}
        </For>
      </div>
      <div class="grp-why">
        kept the copy in {instanceName(app, props.model.keeper?.instanceId)}, the
        first install you registered
        <Show when={blockedCount() > 0}>
          {" "}
          &middot;{" "}
          {blockedCount() === 1
            ? "one copy could not be read"
            : `${blockedCount()} copies could not be read`}
        </Show>
      </div>
    </div>
  );
}

function BlockedRow(props: { entry: BlockedPlacement }) {
  const app = useApp();
  const reason = () => props.entry.reason;
  const name = () => instanceName(app, props.entry.placement.instanceId);

  const recheck = async () => {
    const machine = await app.engine.readMachine();
    await app.actions.refresh();
    if (reason().kind === "file_open" && machine.running.length === 0) {
      app.actions.showToast(
        `Checked · no ComfyUI is running, ${fmt(app.plan()?.totals.reclaimBytes ?? 0)} can now come back`,
      );
    } else if (reason().kind === "file_open") {
      app.actions.showToast(
        `Checked · ${machine.running.map((p) => instanceName(app, p.instanceId)).join(" and ")} is still running`,
        "bad",
      );
    } else {
      app.actions.showToast("Checked · Windows still refuses that folder", "bad");
    }
  };

  const copyInstead = async () => {
    const model = props.entry.model;
    const placement = props.entry.placement;
    const keeperDrive = model.keeper
      ? driveOf(model.keeper.fullPath)
      : driveOf(app.machine()!.vaultPath);
    const vaultDrive = driveOf(app.machine()!.vaultPath);
    await app.engine.markCopyInstead(model.id, placement.id);
    await app.actions.refresh();
    const otherDrive =
      reason().kind === "other_drive"
        ? (reason() as { drive: string }).drive
        : driveOf(placement.fullPath);
    app.actions.showToast(
      keeperDrive === vaultDrive
        ? `Marked to copy instead · drive ${otherDrive} gives back ${fmt(model.bytes)}`
        : `Marked to copy instead · drive ${vaultDrive} gives up ${fmt(model.bytes)} first`,
    );
  };

  return (
    <div class="grp dead">
      <div class="grp-h static">
        <Checkbox on={false} dead />
        <span class="grp-n">{props.entry.model.filename}</span>
        <span class="grp-s">
          {fmt(props.entry.model.bytes)}
          <em>stays</em>
        </span>
      </div>
      <div class="grp-b">
        <div class="cp">
          <span class="role stay">{blockedRole(reason())}</span>
          <span class="who">{name()}</span>
          <span class="pp">{props.entry.placement.folder}</span>
        </div>
      </div>
      <div class="grp-why wide">{blockedWhy(reason(), name())}</div>
      <div class="grp-fix">
        <Show
          when={reason().kind === "other_drive"}
          fallback={
            <button class="btn sm" onClick={() => void recheck()}>
              Re-check
            </button>
          }
        >
          <button class="btn sm" onClick={() => void copyInstead()}>
            Copy it instead
          </button>
        </Show>
      </div>
    </div>
  );
}

function BlockerPanel() {
  const app = useApp();
  const gate = () => app.gate();
  const blockers = () => gateBlockers(gate());

  const recheckDeveloperMode = async () => {
    const machine = await app.engine.readMachine();
    await app.actions.refresh();
    app.actions.showToast(
      machine.developerMode
        ? "Checked · Developer Mode is on, links can be made"
        : "Checked · Developer Mode is still off",
      machine.developerMode ? "ok" : "bad",
    );
  };

  const recheckComfy = async () => {
    const machine = await app.engine.readMachine();
    await app.actions.refresh();
    if (machine.running.length === 0) {
      app.actions.showToast(
        `Checked · no ComfyUI is running, ${fmt(app.plan()?.totals.reclaimBytes ?? 0)} can now come back`,
      );
    } else {
      const names = machine.running
        .map((p) => instanceName(app, p.instanceId))
        .join(" and ");
      app.actions.showToast(`Checked · ${names} is still running`, "bad");
    }
  };

  return (
    <div class="blk">
      <h3>
        <Icon name="warn" size={13} />
        Apply is blocked &middot; {blockers().length}{" "}
        {blockers().length === 1 ? "thing" : "things"} to fix
      </h3>
      <For each={blockers()}>
        {(blocker) =>
          blocker.kind === "developer_mode_off" ? (
            <div class="blkrow">
              <div class="bl">
                <div class="bt">Windows Developer Mode is off</div>
                <div class="bd">
                  Windows only lets this program create a link while it is on. A
                  link is what keeps ComfyUI working after a file moves.
                </div>
                <div class="where">
                  Settings &nbsp;&rsaquo;&nbsp; System &nbsp;&rsaquo;&nbsp; For
                  developers &nbsp;&rsaquo;&nbsp; Developer Mode
                </div>
              </div>
              <div class="ba">
                <button
                  class="btn sm"
                  onClick={() => void app.engine.openWindowsDeveloperSettings()}
                >
                  <Icon name="external" size={11} />
                  Open that page
                </button>
                <button class="btn sm pri" onClick={() => void recheckDeveloperMode()}>
                  <Icon name="refresh" size={11} />
                  Check again
                </button>
              </div>
            </div>
          ) : (
            <div class="blkrow">
              <div class="bl">
                <div class="bt">
                  {blocker.processes
                    .map((p) => `ComfyUI-${instanceName(app, p.instanceId)}`)
                    .join(" and ")}{" "}
                  {blocker.processes.length === 1 ? "is" : "are"} running
                </div>
                <div class="bd">
                  {blocker.processes
                    .map(
                      (p) =>
                        `${p.process}, pid ${p.pid}, holding ${p.openFiles} ${p.openFiles === 1 ? "file" : "files"} open`,
                    )
                    .join("; ")}
                  . Windows will not move a file while a program has it open.{" "}
                  <Show
                    when={app.reclaimIfClosed() > (app.plan()?.totals.reclaimBytes ?? 0)}
                  >
                    Closing{" "}
                    {blocker.processes.length === 1 ? "it" : "them"} takes this run
                    from{" "}
                    <span class="emph">{fmt(app.plan()!.totals.reclaimBytes)}</span>{" "}
                    to <span class="emph">{fmt(app.reclaimIfClosed())}</span>.
                  </Show>
                </div>
              </div>
              <div class="ba">
                <button class="btn sm pri" onClick={() => void recheckComfy()}>
                  <Icon name="refresh" size={11} />
                  Check again
                </button>
              </div>
            </div>
          )
        }
      </For>
      <div class="scope">
        Everything else works, and the report below is complete. Only Apply is held
        back.
      </div>
    </div>
  );
}

function CommitBar() {
  const app = useApp();
  const selection = () => app.selection();
  const gate = () => app.gate();
  const drive = () => app.machine()!.vaultDrive;

  const apply = () => {
    void app.engine.startApply(selection().models.map((m) => m.id));
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
      <span class="n dim">back on drive {drive().letter}</span>
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
