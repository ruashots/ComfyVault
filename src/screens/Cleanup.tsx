import { For, Show, createMemo } from "solid-js";

import { Icon } from "~/components/Icon";
import { EmptyScreen, Header } from "~/components/Shell";
import { Wrap } from "~/components/Wrap";
import { dayMonth, fmt } from "~/domain/format";
import type { PlannedModel } from "~/domain/plan";
import { openConfirm } from "~/modals/confirm";
import { openInstancePicker } from "~/modals/picker";
import { useApp } from "~/state/store";

/**
 * Which name the vault should keep: the one the most workflow files name, and
 * the longer name when they are level. The reason is printed under the choices,
 * so the rule is never a secret.
 */
export function suggestName(model: PlannedModel): {
  name: string;
  reason: string;
} {
  const hits = model.model.workflowHitsByName;
  const scored = model.allNames
    .map((name) => ({ name, score: hits[name] ?? 0 }))
    .sort((a, b) => b.score - a.score || b.name.length - a.name.length);
  const best = scored[0]!;
  const rest = scored.slice(1);

  if (best.score === 0) {
    return {
      name: best.name,
      reason:
        rest.length === 1
          ? "longer name, and neither appears in a workflow file"
          : "longest name, and none of them appears in a workflow file",
    };
  }
  const others = rest.filter((n) => n.score > 0);
  if (others.length === 0) {
    return {
      name: best.name,
      reason: `this name appears in ${best.score} workflow ${best.score === 1 ? "file" : "files"}, ${rest.length === 1 ? "the other does" : "the others do"} not`,
    };
  }
  return {
    name: best.name,
    reason: `this name appears in ${best.score} workflow ${best.score === 1 ? "file" : "files"}, ${rest.length === 1 ? "the other in" : "the next in"} ${others[0]!.score}`,
  };
}

export function CleanupScreen() {
  const app = useApp();
  return (
    <Show
      when={app.hasInstances()}
      fallback={
        <EmptyScreen
          title="Cleanup"
          head="Nothing to tidy yet"
          body="Cleanup works on the vault. Register an install and run a scan first."
        >
          <button class="btn pri" onClick={() => void openInstancePicker(app)}>
            <Icon name="folder" size={13} />
            Choose an install folder
          </button>
        </EmptyScreen>
      }
    >
      <CleanupBody />
    </Show>
  );
}

function CleanupBody() {
  const app = useApp();
  const plan = () => app.plan()!;
  const orphanBytes = createMemo(() =>
    plan().orphans.reduce((sum, m) => sum + m.bytes, 0),
  );

  return (
    <>
      <Header
        title="Cleanup"
        sub={`${plan().aliases.length} name groups · ${plan().orphans.length} unused vault files`}
      />
      <div class="screen">
        <div class="scroll">
          <div class="sec">
            <span class="t">One model, more than one name</span>
            <span class="n">{plan().aliases.length} groups</span>
          </div>
          <Show
            when={plan().aliases.length > 0}
            fallback={
              <div class="note">
                Every file in the vault answers to one name. There is nothing to
                settle.
              </div>
            }
          >
            <div class="note" style={{ margin: "-4px 0 10px", "max-width": "700px" }}>
              These files are byte for byte identical and carry different names.
              Pick the name the vault keeps. The others stay as links by default,
              so any saved workflow that names them still opens.{" "}
              <span class="emph">This frees no disk space.</span> What it gives you
              is one entry per model in ComfyUI&rsquo;s dropdown instead of two.
            </div>
            <For each={plan().aliases}>
              {(model) => <AliasGroup model={model} />}
            </For>
          </Show>

          <div class="sec secgap">
            <span class="t">Nothing points at these</span>
            <span class="n">
              {plan().orphans.length} files &middot; {fmt(orphanBytes())}
            </span>
          </div>
          <Show
            when={plan().orphans.length > 0}
            fallback={
              <div class="note">
                Every file in the vault is linked from at least one install.
              </div>
            }
          >
            <div class="note" style={{ margin: "-4px 0 8px" }}>
              Vault files no install links to. Deleting one does free disk space,
              and it cannot be undone.
            </div>
            <For each={plan().orphans}>
              {(model) => <OrphanRow model={model} />}
            </For>
          </Show>
        </div>
      </div>
    </>
  );
}

function AliasGroup(props: { model: PlannedModel }) {
  const app = useApp();
  const model = () => props.model;
  const suggestion = createMemo(() => suggestName(model()));
  const renaming = () => app.renaming()?.modelId === model().id;

  const instanceUsing = (name: string) =>
    model().model.placements.find((p) => p.filename === name);

  const choose = async (name: string) => {
    if (name === model().filename) return;
    await app.engine.setVaultName(model().id, name);
    await app.actions.refresh();
    app.actions.showToast(`The vault will call it ${name}`);
  };

  const saveTypedName = async () => {
    const value = app.renaming()?.value.trim() ?? "";
    if (!value) return;
    app.actions.cancelRename();
    await app.engine.setVaultName(model().id, value);
    await app.actions.refresh();
    app.actions.showToast(`The vault will call it ${value}`);
  };

  const dropName = (name: string) => {
    openConfirm(app, {
      title: "Remove a name",
      cta: "Remove the name",
      body: [
        [
          { text: name, emph: true },
          {
            text: " stops existing. Any saved workflow that names this file will fail to load it, and ComfyUI will show it as missing. The model itself is not deleted and no disk space is returned.",
          },
        ],
      ],
      action: async () => {
        await app.engine.dropName(model().id, name);
        await app.actions.refresh();
        app.actions.showToast(`Removed ${name}`);
      },
    });
  };

  return (
    <div class="cgrp">
      <div class="ch">
        <span>{model().folder}</span>
        <span class="faint">
          &middot; same bytes, {model().allNames.length} names
        </span>
        <span class="sz">{fmt(model().bytes)}</span>
      </div>

      <For each={model().allNames}>
        {(name) => {
          const chosen = () => name === model().filename;
          const isSuggested = () => name === suggestion().name;
          const source = () => instanceUsing(name);
          return (
            <div class="optrow" classList={{ on: chosen() }}>
              <button
                class="opt"
                role="radio"
                aria-checked={chosen()}
                onClick={() => void choose(name)}
              >
                <span class="radio" />
                <span class="on-n">
                  <span class="nm" title={name}>
                    <Wrap text={name} />
                  </span>
                  <span class="rs">
                    <Show
                      when={source()}
                      fallback={<>a name you typed, new to the vault</>}
                    >
                      {(placement) => (
                        <>
                          the name used in{" "}
                          {app.scan()?.instances.find(
                            (i) => i.id === placement().instanceId,
                          )?.name ?? placement().instanceId}
                        </>
                      )}
                    </Show>
                  </span>
                </span>
                <Show when={isSuggested()}>
                  <span class="tag">suggested</span>
                </Show>
              </button>
              <Show when={!chosen()} fallback={<span class="drop-sp" />}>
                <button
                  class="drop"
                  title="Remove this name"
                  aria-label={`Remove the name ${name}`}
                  onClick={() => dropName(name)}
                >
                  <Icon name="trash" size={12} />
                </button>
              </Show>
            </div>
          );
        }}
      </For>

      <div class="why">Suggested because {suggestion().reason}.</div>

      <Show
        when={renaming()}
        fallback={
          <div class="foot">
            <button
              class="btn sm"
              onClick={() =>
                app.actions.startRename(model().id, suggestion().name)
              }
            >
              <Icon name="file" size={11} />
              Type a different name
            </button>
            <span class="note" style={{ "font-size": "9.5px" }}>
              The names you do not keep stay as links. Remove one only if nothing
              uses it.
            </span>
          </div>
        }
      >
        <div class="renamer">
          <label class="field">
            <Icon name="file" size={12} />
            <input
              value={app.renaming()?.value ?? ""}
              aria-label="A different name for the vault"
              ref={(el) => queueMicrotask(() => el.select())}
              onInput={(e) => app.actions.setRenameValue(e.currentTarget.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault();
                  void saveTypedName();
                }
                if (e.key === "Escape") {
                  e.preventDefault();
                  e.stopPropagation();
                  app.actions.cancelRename();
                }
              }}
            />
          </label>
          <button class="btn sm pri" onClick={() => void saveTypedName()}>
            Use this name
          </button>
          <button class="btn sm" onClick={() => app.actions.cancelRename()}>
            Cancel
          </button>
        </div>
      </Show>
    </div>
  );
}

function OrphanRow(props: { model: PlannedModel }) {
  const app = useApp();
  const removed = () => app.scan()?.removedInstance;

  const remove = () => {
    openConfirm(app, {
      title: "Delete a vault file",
      cta: "Delete it",
      body: [
        [
          { text: props.model.filename, emph: true },
          { text: " is deleted from the vault and " },
          { text: fmt(props.model.bytes), emph: true },
          {
            text: " comes back. Nothing points at it today. This cannot be undone.",
          },
        ],
      ],
      action: async () => {
        await app.engine.deleteOrphan(props.model.id);
        await app.actions.refresh();
        app.actions.showToast(`Deleted · ${fmt(props.model.bytes)} back`);
      },
    });
  };

  return (
    <div class="grp">
      <div class="grp-h static">
        <span class="orphan-ic">
          <Icon name="vault" size={12} />
        </span>
        <span class="grp-n">{props.model.filename}</span>
        <span class="grp-s">{fmt(props.model.bytes)}</span>
        <button class="btn sm dng" onClick={remove}>
          <Icon name="trash" size={11} />
          Delete
        </button>
      </div>
      <div class="grp-why">
        {props.model.folder}
        <Show when={removed()}>
          {(gone) => (
            <>
              {" "}
              &middot; nothing points at it since {gone().name} was removed on{" "}
              {dayMonth(gone().removedAt)}
            </>
          )}
        </Show>
      </div>
    </div>
  );
}
