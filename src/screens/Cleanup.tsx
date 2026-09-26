import { For, Show, createMemo } from "solid-js";

import { Icon } from "~/components/Icon";
import { DanglingLinks, ReplacedLinks } from "~/components/DanglingLinks";
import { EmptyScreen, Header } from "~/components/Shell";
import { Wrap } from "~/components/Wrap";
import { dayMonth, fmt } from "~/domain/format";
import { buildNameGroupView, type NameGroupView } from "~/domain/view";
import { installNameOf } from "~/domain/installname";
import { openConfirm } from "~/modals/confirm";
import { useApp } from "~/state/store";
import type { VaultFile } from "~/ipc/contract";

export function CleanupScreen() {
  const app = useApp();
  return (
    <Show
      when={app.setupDone()}
      fallback={
        <EmptyScreen
          title="Cleanup"
          head="Nothing to tidy yet"
          body={`Cleanup works on the vault. ${app.missingStep() ?? ""}`}
        >
          <button class="btn pri" onClick={() => app.actions.go("home")}>
            <Icon name="arrow" size={13} />
            Finish setting up
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
  const groups = createMemo(() => app.nameGroups().map(buildNameGroupView));
  const orphanBytes = createMemo(() =>
    app.orphans().reduce((sum, f) => sum + f.sizeBytes, 0),
  );

  return (
    <>
      <Header
        title="Cleanup"
        sub={
          app.danglingLinks().length > 0
            ? `${app.danglingLinks().length} broken links · ${groups().length} name groups · ${app.orphans().length} unused vault files`
            : `${groups().length} name groups · ${app.orphans().length} unused vault files`
        }
      />
      <div class="screen">
        <div class="scroll">
          <DanglingLinks />
          <div class="sec">
            <span class="t">One model, more than one name</span>
            <span class="n">{groups().length} groups</span>
          </div>
          <Show
            when={groups().length > 0}
            fallback={
              <div class="note">
                Every file in the vault answers to one name. There is nothing to
                settle. Names appear here after a run brings the same file in under
                two different names.
              </div>
            }
          >
            <div class="note" style={{ margin: "-4px 0 10px", "max-width": "700px" }}>
              These files are byte for byte identical and carry different names. The
              vault keeps one as the real file and the others as links beside it, so
              any saved workflow that names them still opens. Pick the one the vault
              keeps. <span class="emph">This frees no disk space.</span> What it
              gives you is one entry per model in ComfyUI&rsquo;s dropdown instead
              of two.
            </div>
            <For each={groups()}>{(group) => <NameGroupCard view={group} />}</For>
          </Show>

          <div class="sec secgap">
            <span class="t">Nothing points at these</span>
            <span class="n">
              {app.orphans().length} files &middot; {fmt(orphanBytes())}
            </span>
          </div>
          <Show
            when={app.orphans().length > 0}
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
            <For each={app.orphans()}>{(file) => <OrphanRow file={file} />}</For>
          </Show>
          <ReplacedLinks />
        </div>
      </div>
    </>
  );
}

function NameGroupCard(props: { view: NameGroupView }) {
  const app = useApp();
  const group = () => props.view.group;
  const renaming = () => app.renaming()?.sha256 === group().sha256;

  const choose = (name: string) => {
    if (name === group().canonicalName) return;
    void app.actions.run(
      () => app.engine.setCanonicalName(group().sha256, name),
      `The vault keeps ${name}`,
    );
  };

  const saveTypedName = () => {
    const value = app.renaming()?.value.trim() ?? "";
    if (!value) return;
    app.actions.cancelRename();
    void app.actions.run(
      () => app.engine.setCanonicalName(group().sha256, value),
      `The vault keeps ${value}`,
    );
  };

  const removeName = (name: string) => {
    openConfirm(app, {
      title: "Remove a name",
      cta: "Remove the name",
      body: [
        [
          { text: name, emph: true },
          {
            text: " stops existing inside the vault. Any saved workflow that names this file will fail to load it, and ComfyUI will show it as missing. The model itself is not deleted and no disk space is returned.",
          },
        ],
      ],
      action: async () => {
        await app.engine.removeAlias(group().sha256, name);
      },
    });
  };

  return (
    <div class="cgrp">
      <div class="ch">
        <span>{group().category}</span>
        <span class="faint">
          &middot; same bytes, {group().names.length} names
        </span>
        <span class="sz">{fmt(group().sizeBytes)}</span>
      </div>

      <For each={props.view.choices}>
        {(choice) => (
          <div class="optrow" classList={{ on: choice.isCanonical }}>
            <button
              class="opt"
              role="radio"
              aria-checked={choice.isCanonical}
              onClick={() => choose(choice.name)}
            >
              <span class="radio" />
              <span class="on-n">
                <span class="nm" title={choice.name}>
                  <Wrap text={choice.name} />
                </span>
                <span class="rs">
                  <Show
                    when={choice.seenInInstalls.length > 0}
                    fallback={<>a name you typed, new to the vault</>}
                  >
                    the name used in{" "}
                    {choice.seenInInstalls
                      .map((id) => installNameOf(id, app.installs()))
                      .join(" and ")}
                  </Show>
                  <Show when={choice.usedByLinks > 0}>
                    {" "}
                    &middot; {choice.usedByLinks}{" "}
                    {choice.usedByLinks === 1 ? "link points" : "links point"} at it
                  </Show>
                </span>
              </span>
              <Show when={choice.name === props.view.suggestion.name}>
                <span class="tag">suggested</span>
              </Show>
            </button>
            <Show when={choice.removable} fallback={<span class="drop-sp" />}>
              <button
                class="drop"
                title="Remove this name"
                aria-label={`Remove the name ${choice.name}`}
                onClick={() => removeName(choice.name)}
              >
                <Icon name="trash" size={12} />
              </button>
            </Show>
          </div>
        )}
      </For>

      <div class="why">Suggested because {props.view.suggestion.reason}.</div>

      <Show
        when={renaming()}
        fallback={
          <div class="foot">
            <button
              class="btn sm"
              onClick={() =>
                app.actions.startRename(
                  group().sha256,
                  props.view.suggestion.name,
                )
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
                  saveTypedName();
                }
                if (e.key === "Escape") {
                  e.preventDefault();
                  e.stopPropagation();
                  app.actions.cancelRename();
                }
              }}
            />
          </label>
          <button class="btn sm pri" onClick={saveTypedName}>
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

function OrphanRow(props: { file: VaultFile }) {
  const app = useApp();

  const remove = () => {
    openConfirm(app, {
      title: "Delete a vault file",
      cta: "Delete it",
      body: [
        [
          { text: props.file.canonicalName, emph: true },
          { text: " is deleted from the vault and " },
          { text: fmt(props.file.sizeBytes), emph: true },
          {
            text: " comes back. Nothing points at it today. This cannot be undone: the bytes are gone.",
          },
        ],
      ],
      action: async () => {
        await app.engine.deleteVaultFile(props.file.sha256, props.file.sha256);
      },
    });
  };

  return (
    <div class="grp">
      <div class="grp-h static">
        <span class="orphan-ic">
          <Icon name="vault" size={12} />
        </span>
        <span class="grp-n">{props.file.canonicalName}</span>
        <span class="grp-s">{fmt(props.file.sizeBytes)}</span>
        <button class="btn sm dng" onClick={remove}>
          <Icon name="trash" size={11} />
          Delete
        </button>
      </div>
      <div class="grp-why">
        {props.file.category} &middot; in the vault since{" "}
        {dayMonth(props.file.addedAt)}, and no install links to it
      </div>
    </div>
  );
}
