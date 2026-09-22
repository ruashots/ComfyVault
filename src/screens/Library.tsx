import { For, Show, createEffect, createMemo, onCleanup, onMount } from "solid-js";

import { Icon } from "~/components/Icon";
import { EmptyScreen, Header } from "~/components/Shell";
import { Wrap } from "~/components/Wrap";
import { blockedShort } from "~/domain/blocked";
import { dayMonth, fmt, fmtExactMB, mid, shortHash } from "~/domain/format";
import type { Plan, PlannedModel } from "~/domain/plan";
import { openConfirm } from "~/modals/confirm";
import { openInstancePicker, openLinkPicker } from "~/modals/picker";
import { useApp, type LibrarySort } from "~/state/store";

/** Search, filter and sort, in one place so the list and its count agree. */
export function libraryRows(plan: Plan, view: {
  query: string;
  folder: string;
  unusedOnly: boolean;
  sort: LibrarySort;
}): PlannedModel[] {
  const query = view.query.trim().toLowerCase();
  const rows = plan.models.filter((model) => {
    if (view.folder !== "all" && model.folder !== view.folder) return false;
    if (view.unusedOnly && model.model.workflowHits > 0) return false;
    if (query && !model.allNames.join(" ").toLowerCase().includes(query)) {
      return false;
    }
    return true;
  });
  const copies = (m: PlannedModel) => m.model.placements.length;
  rows.sort((a, b) => {
    if (view.sort === "name") return a.filename.localeCompare(b.filename);
    if (view.sort === "links") return copies(b) - copies(a) || b.bytes - a.bytes;
    return b.bytes - a.bytes;
  });
  return rows;
}

export function LibraryScreen() {
  const app = useApp();
  return (
    <Show
      when={app.hasInstances()}
      fallback={
        <EmptyScreen
          title="Library"
          head="The vault is empty"
          body="Register a ComfyUI install and run a scan. Every model file found is listed here once, whatever folder it sits in and however many copies exist."
        >
          <button class="btn pri" onClick={() => void openInstancePicker(app)}>
            <Icon name="folder" size={13} />
            Choose an install folder
          </button>
        </EmptyScreen>
      }
    >
      <LibraryList />
    </Show>
  );
}

function LibraryList() {
  const app = useApp();
  const plan = () => app.plan()!;
  const rows = createMemo(() => libraryRows(plan(), app.lib));

  /** A filter that hides the open model closes the drawer rather than jumping. */
  createEffect(() => {
    const selected = app.lib.selected;
    if (selected != null && !rows().some((m) => m.id === selected)) {
      app.setLib({ selected: null, drawerOpen: false });
    }
  });

  const selected = createMemo(() =>
    app.lib.selected == null
      ? null
      : (plan().byId.get(app.lib.selected) ?? null),
  );
  const drawerOpen = () => app.lib.drawerOpen && selected() != null;
  const narrow = () => drawerOpen();

  let listEl: HTMLDivElement | undefined;

  const move = (delta: number) => {
    const list = rows();
    if (list.length === 0) return;
    const at = list.findIndex((m) => m.id === app.lib.selected);
    const next = at < 0 ? 0 : Math.max(0, Math.min(list.length - 1, at + delta));
    app.setLib("selected", list[next]!.id);
    queueMicrotask(() => {
      listEl?.querySelector(".lrow.on")?.scrollIntoView({ block: "nearest" });
    });
  };

  const onKeyDown = (event: KeyboardEvent) => {
    if (app.modal()) return;
    const target = event.target as HTMLElement | null;
    const typing = target?.tagName === "INPUT";
    if (event.key === "ArrowDown") {
      event.preventDefault();
      move(1);
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      move(-1);
    } else if (
      event.key === "Enter" &&
      app.lib.selected != null &&
      !app.lib.drawerOpen &&
      !typing
    ) {
      event.preventDefault();
      app.setLib("drawerOpen", true);
    }
  };

  onMount(() => {
    window.addEventListener("keydown", onKeyDown);
    onCleanup(() => window.removeEventListener("keydown", onKeyDown));
  });

  const totalShown = createMemo(() =>
    rows().reduce((sum, m) => sum + m.bytes, 0),
  );

  return (
    <>
      <Header
        title="Library"
        sub={`${plan().models.length} models · counted once each`}
      />
      <div class="screen">
        <div class="toolbar">
          <label class="field">
            <Icon name="search" size={12} />
            <input
              id="libq"
              placeholder="Search a filename"
              value={app.lib.query}
              aria-label="Search a filename"
              onInput={(e) => app.setLib("query", e.currentTarget.value)}
            />
          </label>
          <FolderMenu />
          <button
            class="chip"
            classList={{ on: app.lib.unusedOnly }}
            aria-pressed={app.lib.unusedOnly}
            onClick={() => app.setLib("unusedOnly", !app.lib.unusedOnly)}
          >
            Not used &middot; {plan().totals.unused}
          </button>
          <span style={{ flex: 1 }} />
          <span class="count">
            {rows().length} of {plan().models.length} &middot; {fmt(totalShown())}
          </span>
        </div>

        <div class="split">
          <div class="lib-list" ref={listEl}>
            <div class="lib-head">
              <span class="h-dot" />
              <SortButton column="name" label="Model" class="h-name" />
              <Show when={!narrow()}>
                <span class="h-folder">Folder</span>
              </Show>
              <SortButton column="size" label="Size" class="h-size" />
              <Show when={!narrow()}>
                <button
                  class="h-links"
                  classList={{ on: app.lib.sort === "links" }}
                  title="Sort by how many places hold it"
                  aria-label="Sort by how many places hold it"
                  onClick={() => app.setLib("sort", "links")}
                >
                  <Icon name="link" size={11} />
                </button>
              </Show>
              <span class="h-go" />
            </div>

            <For each={rows()}>
              {(model) => (
                <button
                  class="lrow"
                  classList={{ on: model.id === app.lib.selected }}
                  title={model.filename}
                  onClick={() =>
                    app.setLib({ selected: model.id, drawerOpen: true })
                  }
                >
                  <span
                    class="dot"
                    classList={{
                      used: model.model.workflowHits > 0,
                      unused: model.model.workflowHits === 0,
                    }}
                  />
                  <span class="ln">{mid(model.filename, narrow() ? 34 : 72)}</span>
                  <Show when={!narrow()}>
                    <span class="lf">{model.folder}</span>
                  </Show>
                  <span class="lz">{fmt(model.bytes)}</span>
                  <Show when={!narrow()}>
                    <span
                      class="lk"
                      classList={{ none: model.model.placements.length === 0 }}
                    >
                      {model.model.placements.length}
                    </span>
                  </Show>
                  <span class="go">
                    <Icon name="arrow" size={12} />
                  </span>
                </button>
              )}
            </For>

            <Show when={rows().length === 0}>
              <div class="lib-empty">
                <div class="lbl">Nothing matches</div>
                <div class="note">
                  No model here is named{" "}
                  {app.lib.query ? `“${app.lib.query}”` : "that"}
                  {app.lib.folder !== "all" ? ` inside ${app.lib.folder}` : ""}
                  {app.lib.unusedOnly ? " and unused" : ""}.
                </div>
                <div class="acts">
                  <button
                    class="btn sm"
                    onClick={() =>
                      app.setLib({
                        query: "",
                        folder: "all",
                        unusedOnly: false,
                        sort: "size",
                      })
                    }
                  >
                    Clear the filters
                  </button>
                </div>
              </div>
            </Show>
          </div>

          <Show when={drawerOpen()}>
            <Drawer model={selected()!} />
          </Show>
        </div>
      </div>
    </>
  );
}

function SortButton(props: { column: LibrarySort; label: string; class: string }) {
  const app = useApp();
  return (
    <button
      class={props.class}
      classList={{ on: app.lib.sort === props.column }}
      aria-pressed={app.lib.sort === props.column}
      onClick={() => app.setLib("sort", props.column)}
    >
      {props.label}
      {app.lib.sort === props.column ? " ↓" : ""}
    </button>
  );
}

function FolderMenu() {
  const app = useApp();
  const plan = () => app.plan()!;
  const items = createMemo(() => [
    { key: "all", label: "All folders", count: plan().models.length },
    ...plan().folders.map((folder) => ({
      key: folder,
      label: folder,
      count: plan().models.filter((m) => m.folder === folder).length,
    })),
  ]);

  return (
    <span class="menu-wrap">
      <button
        class="sel"
        classList={{ on: app.lib.folder !== "all" }}
        aria-haspopup="menu"
        aria-expanded={app.folderMenuOpen()}
        onClick={(e) => {
          e.stopPropagation();
          app.actions.setFolderMenuOpen(!app.folderMenuOpen());
        }}
      >
        <Icon name="folder" size={12} />
        <span>{app.lib.folder === "all" ? "All folders" : app.lib.folder}</span>
        <Icon name="chev" size={11} />
      </button>
      <Show when={app.folderMenuOpen()}>
        <div class="menu" role="menu">
          <For each={items()}>
            {(item) => (
              <button
                class="mitem"
                classList={{ on: app.lib.folder === item.key }}
                role="menuitemradio"
                aria-checked={app.lib.folder === item.key}
                onClick={() => {
                  app.setLib("folder", item.key);
                  app.actions.setFolderMenuOpen(false);
                }}
              >
                <span>{item.label}</span>
                <span class="c">{item.count}</span>
              </button>
            )}
          </For>
        </div>
      </Show>
    </span>
  );
}

function Drawer(props: { model: PlannedModel }) {
  const app = useApp();
  return (
    <div class="drawer">
      <div class="dhead">
        <div class="dt">
          <div class="det-name" title={props.model.filename}>
            <Wrap text={props.model.filename} />
          </div>
          <div class="det-meta">
            {props.model.folder} &nbsp;&middot;&nbsp; {fmt(props.model.bytes)}{" "}
            &nbsp;&middot;&nbsp;
            <Show
              when={props.model.model.workflowHits > 0}
              fallback={<span style={{ color: "var(--t-muted)" }}>Not used</span>}
            >
              <span class="grn">In use</span>
            </Show>
          </div>
        </div>
        <button
          class="dclose"
          title="Close, or press Esc"
          aria-label="Close the details"
          onClick={() => app.setLib("drawerOpen", false)}
        >
          <Icon name="x" size={13} />
        </button>
      </div>
      <div class="dbody">
        <DrawerBody model={props.model} />
      </div>
    </div>
  );
}

function DrawerBody(props: { model: PlannedModel }) {
  const app = useApp();
  const model = () => props.model;
  const removed = () => app.scan()?.removedInstance;

  const instanceName = (id: string) =>
    app.scan()?.instances.find((i) => i.id === id)?.name ?? id;

  const deleteOrphan = () => {
    openConfirm(app, {
      title: "Delete a vault file",
      cta: "Delete it",
      body: [
        [
          { text: model().filename, emph: true },
          { text: " is deleted from the vault and " },
          { text: fmt(model().bytes), emph: true },
          {
            text: " comes back. Nothing points at it today. This cannot be undone.",
          },
        ],
      ],
      action: async () => {
        await app.engine.deleteOrphan(model().id);
        await app.actions.refresh();
        app.setLib({ selected: null, drawerOpen: false });
        app.actions.showToast(`Deleted · ${fmt(model().bytes)} back`);
      },
    });
  };

  return (
    <>
      <div class="det-acts">
        <button
          class="btn sm"
          onClick={() => void openLinkPicker(app, model().id)}
        >
          <Icon name="plus" size={11} />
          Link into an instance
        </button>
        <Show when={model().isOrphan}>
          <button class="btn sm dng" onClick={deleteOrphan}>
            <Icon name="trash" size={11} />
            Delete
          </button>
        </Show>
      </div>

      <div class="sec" style={{ "margin-top": "14px" }}>
        <span class="t">Where it reaches</span>
        <span class="n">{model().model.placements.length || "none"}</span>
      </div>
      <Show
        when={model().model.placements.length > 0}
        fallback={
          <div class="note">
            Nothing points at this file.
            <Show when={removed()}>
              {(gone) => (
                <>
                  {" "}
                  It is in the vault because {gone().name} used it, and that install
                  was removed on {dayMonth(gone().removedAt)}.
                </>
              )}
            </Show>{" "}
            Link it into an install, or delete it and get {fmt(model().bytes)} back.
          </div>
        }
      >
        <div class="reach">
          <For each={model().model.placements}>
            {(placement) => (
              <div class="r">
                <div class="top">
                  <Icon
                    name={
                      placement.blocked
                        ? "file"
                        : placement === model().keeper && !placement.isLink
                          ? "vault"
                          : "link"
                    }
                    size={12}
                  />
                  <span>{instanceName(placement.instanceId)}</span>
                  <span class="sp" />
                  <Show
                    when={placement.blocked}
                    fallback={
                      <Show
                        when={placement.isLink}
                        fallback={
                          <span class="pill pend">
                            {placement === model().keeper
                              ? "becomes the vault copy"
                              : "becomes a link"}
                          </span>
                        }
                      >
                        <span class="pill link">link</span>
                      </Show>
                    }
                  >
                    {(blocked) => (
                      <span class="pill bad">{blockedShort(blocked())}</span>
                    )}
                  </Show>
                </div>
                <div class="pp">
                  <Wrap text={placement.fullPath} />
                </div>
                <Show when={placement.filename !== model().filename}>
                  <div class="pp alt">filename here: {placement.filename}</div>
                </Show>
              </div>
            )}
          </For>
        </div>
      </Show>

      <div class="sec secgap plain">
        <span class="t">In the vault</span>
      </div>
      <div class="kv st">
        <span class="k">Path</span>
        <span class="v">
          <Wrap text={model().vaultPath} />
        </span>
      </div>
      <div class="kv">
        <span class="k w96">Size</span>
        <span class="v">{fmtExactMB(model().bytes)}</span>
      </div>
      <div class="kv">
        <span class="k w96">SHA-256</span>
        <span class="v faint" style={{ "font-size": "10px" }}>
          {shortHash(model().model.sha256)}
        </span>
      </div>
      <div class="kv st">
        <span class="k">Added to the vault</span>
        <span class="v">
          {model().model.inVaultSince
            ? dayMonth(model().model.inVaultSince!)
            : "not yet, this plan has not been applied"}
        </span>
      </div>

      <div class="sec secgap plain">
        <span class="t">Used by a workflow</span>
      </div>
      <Show
        when={model().model.workflowHits > 0}
        fallback={
          <div class="note">
            This filename appears in <span class="emph">no workflow file</span>.
            That only means no saved workflow names it. A node could still load it
            from somewhere ComfyVault cannot read.
          </div>
        }
      >
        <div class="note">
          This filename appears in{" "}
          <span class="emph">
            {model().model.workflowHits} workflow{" "}
            {model().model.workflowHits === 1 ? "file" : "files"}
          </span>{" "}
          across your installs.
        </div>
      </Show>

      <Show when={model().allNames.length > 1}>
        <div class="sec secgap plain">
          <span class="t">Other names for these bytes</span>
          <span class="n">{model().allNames.length}</span>
        </div>
        <For each={model().allNames}>
          {(name) => (
            <div class="kv namerow">
              <span class="v">
                {name}
                <Show when={name === model().filename}>
                  {" "}
                  <span class="faint">vault name</span>
                </Show>
              </span>
            </div>
          )}
        </For>
        <div class="note up">
          The same file is stored under more than one name. Cleanup can settle on
          one.
        </div>
      </Show>

      <div class="sec secgap plain">
        <span class="t">Civitai</span>
      </div>
      <Show
        when={model().model.civitai}
        fallback={
          <Show
            when={app.scan()?.civitaiEnabled}
            fallback={
              <div class="note">
                Civitai lookup is off, so nothing was asked about this file.
                ComfyVault works the same either way. Turn it on in Settings.
              </div>
            }
          >
            <div class="note">
              Civitai has no file with this hash. That is normal: official releases
              and anything you built or renamed yourself will never match.
              ComfyVault works the same either way.
            </div>
          </Show>
        }
      >
        {(civitai) => (
          <>
            <div class="kv">
              <span class="k w96">Name</span>
              <span class="v">
                <b>{civitai().name}</b>
              </span>
            </div>
            <div class="kv">
              <span class="k w96">Version</span>
              <span class="v">{civitai().version}</span>
            </div>
            <div class="kv">
              <span class="k w96">Type</span>
              <span class="v">
                {civitai().type} &middot; {civitai().baseModel}
              </span>
            </div>
            <div class="kv">
              <span class="k w96">Uploaded by</span>
              <span class="v">{civitai().uploader}</span>
            </div>
            <div class="note up">
              Returned by Civitai for this file&rsquo;s hash. ComfyVault stores
              nothing else about it.
            </div>
          </>
        )}
      </Show>
    </>
  );
}
