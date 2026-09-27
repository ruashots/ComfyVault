import { For, Show, createMemo, createSignal } from "solid-js";

import { Icon } from "~/components/Icon";
import { DanglingLinks, ReplacedLinks } from "~/components/DanglingLinks";
import { EmptyScreen, Header } from "~/components/Shell";
import { Wrap } from "~/components/Wrap";
import { dayMonth, fmt, joinPath } from "~/domain/format";
import {
  buildNameGroupView,
  usageOfModel,
  type ModelUsage,
  type NameGroupView,
} from "~/domain/view";
import { installNameOf } from "~/domain/installname";
import { openConfirm } from "~/modals/confirm";
import { messageOf, useApp, type AppStore } from "~/state/store";
import type { Install, VaultFile } from "~/ipc/contract";

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
  // A name is used by the installs whose links carry it, whatever the vault file is called.
  const groups = createMemo(() =>
    app
      .nameGroups()
      .map((g) =>
        buildNameGroupView(g, app.vaultFiles().find((f) => f.sha256 === g.sha256)?.links ?? []),
      ),
  );
  const orphanBytes = createMemo(() =>
    app.orphans().reduce((sum, f) => sum + f.sizeBytes, 0),
  );

  return (
    <>
      <Header
        title="Cleanup"
        sub={cleanupSummary(
          app.danglingLinks().length,
          groups().length,
          app.orphans().length,
          app.health()?.stoppedDeletes.length ?? 0,
        )}
      />
      <div class="screen">
        <div class="scroll">
          <DanglingLinks />
          <StoppedDeletes />
          <div class="sec">
            <span class="t">One model with more than one name</span>
            <span class="n">
              {groups().length} {groups().length === 1 ? "model" : "models"}
            </span>
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
              Each model below is one file that your installs know under more than
              one name. The vault file carries one of the names. Each install keeps
              the name it uses now, as its link, so its workflows still open.{" "}
              <span class="emph">Choosing a name here frees no disk space.</span>
            </div>
            <For each={groups()}>{(group) => <NameGroupCard view={group} />}</For>
          </Show>

          <div class="sec secgap">
            <span class="t">Vault files that nothing links to</span>
            <span class="n">
              <Show when={app.orphans().length > 0} fallback="none">
                {app.orphans().length} {app.orphans().length === 1 ? "file" : "files"},{" "}
                {fmt(orphanBytes())}
              </Show>
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
          <DeleteModels />
        </div>
      </div>
    </>
  );
}

/** The top bar's line: what Cleanup found, as sentences. */
export function cleanupSummary(
  broken: number,
  named: number,
  unlinked: number,
  stopped = 0,
): string {
  const parts: string[] = [];
  if (stopped > 0) {
    parts.push(
      stopped === 1 ? "1 delete stopped part way." : `${stopped} deletes stopped part way.`,
    );
  }
  if (broken > 0) {
    parts.push(broken === 1 ? "1 link leads to nothing." : `${broken} links lead to nothing.`);
  }
  parts.push(
    named === 0
      ? "Every model has one name."
      : named === 1
        ? "1 model has more than one name."
        : `${named} models have more than one name.`,
  );
  parts.push(
    unlinked === 0
      ? "Every vault file is linked."
      : unlinked === 1
        ? "1 vault file is not linked from any install."
        : `${unlinked} vault files are not linked from any install.`,
  );
  return parts.join(" ");
}

function NameGroupCard(props: { view: NameGroupView }) {
  const app = useApp();
  const group = () => props.view.group;

  const choose = (name: string) => {
    if (name === group().canonicalName) return;
    void app.actions.run(
      () => app.engine.setCanonicalName(group().sha256, name),
      `The vault file is now named ${name}.`,
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
            text: " is removed from the vault, where it is only a link beside the file. Every install keeps its links and their names, so nothing in ComfyUI changes. The model is not deleted and no disk space is freed.",
          },
        ],
      ],
      action: async () => {
        await app.engine.removeAlias(group().sha256, name);
      },
    });
  };

  /** "ComfyUI-Studio and ComfyUI-Sandbox use this name." */
  const usedBy = (ids: readonly string[]) =>
    ids.length === 0
      ? "No install uses this name."
      : `${ids.map((id) => installNameOf(id, app.installs())).join(" and ")} ${ids.length === 1 ? "uses" : "use"} this name.`;

  return (
    <div class="cgrp">
      <div class="ch">
        <span>{group().category}:</span>
        <span class="faint">the same file under {group().names.length} names</span>
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
                  {choice.isCanonical
                    ? "The vault file has this name now."
                    : "The vault keeps this name as a link beside the file."}{" "}
                  {usedBy(choice.installIds)}
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

      <div class="why">{props.view.suggestion.reason}</div>
      <div class="why">
        Choosing a name renames the file in the vault and keeps{" "}
        {group().names.length > 2 ? "the other names as links" : "the other name as a link"}{" "}
        beside it. No install changes, and no disk space is freed.
      </div>
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
          { text: " will be deleted from the vault, and " },
          { text: fmt(props.file.sizeBytes), emph: true },
          {
            text: " will be freed. Nothing links to it today. This cannot be undone: the bytes are gone.",
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
        in {props.file.category}, in the vault since {dayMonth(props.file.addedAt)}, and
        no install links to it
      </div>
    </div>
  );
}

/** How many of the biggest models show before "Show all". */
const FIRST_SHOWN = 12;

/**
 * Every model the installs link to, biggest first, each with one button that
 * deletes it from the vault and removes every link to it. A file nothing links
 * to is already listed above with its own Delete, so it is not listed twice.
 */
function DeleteModels() {
  const app = useApp();
  const [onlyUnused, setOnlyUnused] = createSignal(false);
  const [showAll, setShowAll] = createSignal(false);

  const models = createMemo(() =>
    app
      .vaultFiles()
      .filter((f) => f.linkCount > 0)
      .map((file) => ({ file, usage: usageOfModel(file, app.usage()) }))
      .sort((a, b) => b.file.sizeBytes - a.file.sizeBytes),
  );
  const total = createMemo(() => models().reduce((sum, m) => sum + m.file.sizeBytes, 0));
  // Nothing was checked when no name was searched at all, and then no model
  // can be called unused.
  const checked = createMemo(() => models().some((m) => m.usage.searched === true));
  const unused = createMemo(() =>
    models().filter((m) => m.usage.searched === true && m.usage.matches.length === 0),
  );
  const listed = createMemo(() => (onlyUnused() && checked() ? unused() : models()));
  const shown = createMemo(() => (showAll() ? listed() : listed().slice(0, FIRST_SHOWN)));

  return (
    <>
      <div class="sec secgap">
        <span class="t">Delete a model</span>
        <span class="n">
          {models().length} {models().length === 1 ? "model" : "models"} in the vault,{" "}
          {fmt(total())}
        </span>
      </div>
      <Show
        when={models().length > 0}
        fallback={<div class="note">No model in the vault is linked from an install.</div>}
      >
        <div class="note lead">
          Deleting a model frees its space and removes every link to it, in every
          install.
        </div>
        <Show when={checked()}>
          <div class="vtools">
            <button
              class="chip"
              classList={{ on: onlyUnused() }}
              aria-pressed={onlyUnused()}
              onClick={() => setOnlyUnused(!onlyUnused())}
            >
              Only models no saved workflow uses ({unused().length})
            </button>
          </div>
          <Show when={onlyUnused()}>
            <div class="lib-method box" role="note">
              {app.usageMethod()} A workflow you never saved lives in the browser,
              where ComfyVault cannot see it, so this list is not a list of models
              that are safe to delete.
            </div>
          </Show>
        </Show>
        <Show
          when={shown().length > 0}
          fallback={
            <div class="note" style={{ padding: "8px 0" }}>
              Every model in the vault is named in a saved workflow.
            </div>
          }
        >
          <For each={shown()}>{(m) => <ModelRow file={m.file} usage={m.usage} />}</For>
        </Show>
        <Show when={listed().length > FIRST_SHOWN}>
          <div class="more">
            <button class="btn sm" onClick={() => setShowAll(!showAll())}>
              {showAll() ? `Show the ${FIRST_SHOWN} biggest only` : `Show all ${listed().length}`}
            </button>
          </div>
        </Show>
      </Show>
    </>
  );
}

/** The installs a model's links are in, each once, by the name shown for it. */
function installsOf(file: VaultFile, installs: readonly Install[]): string[] {
  return [
    ...new Set(
      file.links.map((l) =>
        installs.some((i) => i.id === l.installId)
          ? installNameOf(l.installId, installs)
          : "an install no longer in the list",
      ),
    ),
  ];
}

function ModelRow(props: { file: VaultFile; usage: ModelUsage }) {
  const app = useApp();
  const installs = () => installsOf(props.file, app.installs());
  const used = () => props.usage.matches.length;

  const remove = () => {
    const file = props.file;
    const n = file.links.length;
    const where = installs();
    const usage = props.usage;
    const usageLine =
      usage.searched === null
        ? []
        : !usage.searched
          ? [[{ text: usage.method ?? "" }]]
          : usage.matches.length === 0
            ? [
                [
                  {
                    text: "No saved workflow names it. A workflow you never saved lives in the browser, where ComfyVault cannot see it.",
                  },
                ],
              ]
            : [
                [
                  {
                    text: `It is named in ${usage.matches.length === 1 ? "a saved workflow" : `${usage.matches.length} saved workflows`}: `,
                  },
                  {
                    text: usage.matches
                      .map((m) => `${m.workflowName} (${installNameOf(m.installId, app.installs())})`)
                      .join(", "),
                    emph: true,
                  },
                  {
                    text: `. ${usage.matches.length === 1 ? "It" : "They"} will show the model as missing.`,
                  },
                ],
              ];
    openConfirm(app, {
      title: "Delete a model",
      cta: n === 1 ? "Delete the model and its link" : `Delete the model and its ${n} links`,
      body: [
        [
          { text: file.canonicalName, emph: true },
          { text: " will be deleted, and " },
          { text: fmt(file.sizeBytes), emph: true },
          { text: ` will be freed on drive ${app.vaultVolume()}.` },
        ],
        [
          { text: "This is the only copy ComfyVault knows of. " },
          { text: "This cannot be undone.", emph: true },
          { text: " To use the model again, you must download it again." },
        ],
        [
          {
            text: `${n === 1 ? "Its link is" : `Its ${n} links are`} removed too, so it disappears from ${where.length <= 2 ? where.join(" and ") : "these installs"}:`,
          },
        ],
      ],
      list: file.links.map((l) => l.absPath),
      after: usageLine,
      action: () => deleteWithLinks(app, file),
    });
  };

  return (
    <div class="vrow">
      <div class="vl">
        <span
          class="grp-n"
          title={joinPath(app.vault()?.root ?? "", props.file.vaultRelPath)}
        >
          {props.file.canonicalName}
        </span>
        <div class="vmeta">
          <span>in {props.file.category},</span>
          <span class="lk" title={props.file.links.map((l) => l.absPath).join("\n")}>
            linked in {installs().join(" and ")}
            <Show when={props.usage.searched === true}>,</Show>
          </span>
          <Show when={props.usage.searched === true}>
            <span class="wf">
              <span class="dot" classList={{ used: used() > 0, unused: used() === 0 }} />
              {used() === 0
                ? "used by no saved workflow"
                : `used by ${used()} saved ${used() === 1 ? "workflow" : "workflows"}`}
            </span>
          </Show>
        </div>
      </div>
      <span class="grp-s">{fmt(props.file.sizeBytes)}</span>
      <button
        class="drop"
        title="Delete this model"
        aria-label={`Delete ${props.file.canonicalName}`}
        onClick={remove}
      >
        <Icon name="trash" size={13} />
      </button>
    </div>
  );
}

/** Delete a model and every link to it, then say what that freed. */
async function deleteWithLinks(app: AppStore, file: VaultFile): Promise<void> {
  const done = await app.engine.deleteVaultFile(file.sha256, file.sha256, true);
  const k = done.linksRemoved.length;
  app.actions.showToast(
    `Deleted ${file.canonicalName}. ${fmt(done.bytesFreed)} freed, ${k} ${k === 1 ? "link" : "links"} removed.`,
  );
}

/**
 * Models whose delete was cut off, by the power going say. Some installs lost
 * their link and no longer load the model, and the model is still in the
 * vault, so the delete is half done until it is finished.
 */
function StoppedDeletes() {
  const app = useApp();
  const files = () => app.health()?.stoppedDeletes ?? [];

  const finish = async (file: VaultFile) => {
    let left: string[];
    try {
      left = (await app.engine.listLinks({ sha256: file.sha256 }))
        .filter((l) => l.state === "ok")
        .map((l) => l.absPath);
    } catch (error) {
      app.actions.showToast(messageOf(error), "bad");
      return;
    }
    const installs = installsOf(
      { ...file, links: file.links.filter((l) => left.includes(l.absPath)) },
      app.installs(),
    );
    openConfirm(app, {
      title: "Finish a delete",
      cta: "Finish the delete",
      body: [
        [
          { text: file.canonicalName, emph: true },
          { text: " will be deleted, and " },
          { text: fmt(file.sizeBytes), emph: true },
          { text: ` will be freed on drive ${app.vaultVolume()}.` },
        ],
        [
          { text: "This is the only copy ComfyVault knows of. " },
          { text: "This cannot be undone.", emph: true },
          { text: " To use the model again, you must download it again." },
        ],
        [
          {
            text:
              left.length === 0
                ? "No install links to it any more."
                : `${left.length === 1 ? "Its last link is" : `Its ${left.length} remaining links are`} removed too, so it disappears from ${installs.length <= 2 ? installs.join(" and ") : "these installs"}:`,
          },
        ],
      ],
      list: left,
      action: () => deleteWithLinks(app, file),
    });
  };

  return (
    <Show when={files().length > 0}>
      <div class="blk" role="alert">
        <h3>
          <Icon name="warn" size={13} />
          {files().length === 1
            ? "A delete stopped part way"
            : `${files().length} deletes stopped part way`}
        </h3>
        <div class="blkrow">
          <div class="bl">
            <div class="bt">Finish {files().length === 1 ? "it" : "each one"}</div>
            <div class="bd">
              ComfyVault stopped while it was deleting{" "}
              {files().length === 1 ? "this model" : "these models"}, for example
              because the power went. Some installs already lost their link, so they
              no longer load it. The model is still in the vault and still takes its
              space. Finishing the delete removes it and the links it still has.
            </div>
          </div>
        </div>
        <For each={files()}>
          {(file) => (
            <div class="blkrow">
              <div class="bl">
                <div class="bt">{file.canonicalName}</div>
                <div class="bd">
                  in {file.category}, {fmt(file.sizeBytes)} still in the vault
                </div>
              </div>
              <div class="ba">
                <button class="btn sm dng" onClick={() => void finish(file)}>
                  <Icon name="trash" size={11} />
                  Finish the delete
                </button>
              </div>
            </div>
          )}
        </For>
      </div>
    </Show>
  );
}
