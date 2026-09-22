import {
  For,
  Show,
  createEffect,
  createMemo,
  createResource,
  onCleanup,
  onMount,
} from "solid-js";

import { Icon } from "~/components/Icon";
import { EmptyScreen, Header } from "~/components/Shell";
import { Wrap } from "~/components/Wrap";
import { blockedShort, blockedWhy } from "~/domain/blocked";
import { dayMonth, fmt, fmtExactMB, mid, shortHash } from "~/domain/format";
import { placesOf } from "~/domain/view";
import { ThumbnailNoteForModel } from "~/components/ThumbnailNote";
import { openConfirm } from "~/modals/confirm";
import { openInstallPicker, openLinkPicker } from "~/modals/picker";
import { useApp, type LibrarySort } from "~/state/store";
import { nothingWasSearched } from "~/ipc/contract";
import type { ContentRow, UsageResult } from "~/ipc/contract";

export interface LibraryFilters {
  query: string;
  category: string;
  unusedOnly: boolean;
  sort: LibrarySort;
}

/** Search, filter and sort, in one place so the list and its count agree. */
/**
 * True only when the engine actually looked and found nothing. When there was
 * no saved workflow file to search, there is no answer to act on.
 */
export function isUnused(result: UsageResult | undefined): boolean {
  return result !== undefined && !result.used && !nothingWasSearched(result);
}

/** Every category present in the rows on hand, for the folder filter. */
export function categoriesOf(rows: readonly ContentRow[]): string[] {
  return [...new Set(rows.map((r) => r.category).filter(Boolean))].sort();
}

export function libraryRows(
  rows: readonly ContentRow[],
  view: LibraryFilters,
  usage: ReadonlyMap<string, UsageResult>,
): ContentRow[] {
  const query = view.query.trim().toLowerCase();
  const out = rows.filter((row) => {
    if (view.category !== "all" && row.category !== view.category) return false;
    if (view.unusedOnly && !isUnused(usage.get(row.name))) return false;
    if (
      query &&
      ![row.name, ...row.aliases].join(" ").toLowerCase().includes(query)
    ) {
      return false;
    }
    return true;
  });
  out.sort((a, b) => {
    if (view.sort === "name") return a.name.localeCompare(b.name);
    if (view.sort === "links") {
      return b.occurrenceCount - a.occurrenceCount || b.sizeBytes - a.sizeBytes;
    }
    return b.sizeBytes - a.sizeBytes;
  });
  return out;
}

export function LibraryScreen() {
  const app = useApp();
  return (
    <Show
      when={app.hasInstalls() && app.library().length > 0}
      fallback={
        <EmptyScreen
          title="Library"
          head="The vault is empty"
          body="Register a ComfyUI install and run a scan. Every model file found is listed here once, whatever folder it sits in and however many copies exist."
        >
          <Show
            when={app.hasInstalls()}
            fallback={
              <button class="btn pri" onClick={() => void openInstallPicker(app)}>
                <Icon name="folder" size={13} />
                Choose an install folder
              </button>
            }
          >
            <button
              class="btn pri"
              onClick={() => void app.actions.run(() => app.engine.startScan())}
            >
              <Icon name="scan" size={13} />
              Scan now
            </button>
          </Show>
        </EmptyScreen>
      }
    >
      <LibraryList />
    </Show>
  );
}

function LibraryList() {
  const app = useApp();
  const rows = createMemo(() => libraryRows(app.library(), app.lib, app.usage()));
  const categories = createMemo(() => categoriesOf(app.library()));

  /** A filter that hides the open model closes the drawer rather than jumping. */
  createEffect(() => {
    const selected = app.lib.selected;
    if (selected != null && !rows().some((r) => r.sha256 === selected)) {
      app.setLib({ selected: null, drawerOpen: false });
    }
  });

  const selected = createMemo(() =>
    app.lib.selected == null
      ? null
      : (app.library().find((r) => r.sha256 === app.lib.selected) ?? null),
  );
  const drawerOpen = () => app.lib.drawerOpen && selected() != null;
  const narrow = () => drawerOpen();

  let listEl: HTMLDivElement | undefined;

  const move = (delta: number) => {
    const list = rows();
    if (list.length === 0) return;
    const at = list.findIndex((r) => r.sha256 === app.lib.selected);
    const next = at < 0 ? 0 : Math.max(0, Math.min(list.length - 1, at + delta));
    app.setLib("selected", list[next]!.sha256);
    queueMicrotask(() => {
      listEl?.querySelector(".lrow.on")?.scrollIntoView({ block: "nearest" });
    });
  };

  const onKeyDown = (event: KeyboardEvent) => {
    if (app.modal()) return;
    const typing = (event.target as HTMLElement | null)?.tagName === "INPUT";
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

  const shownBytes = createMemo(() =>
    rows().reduce((sum, r) => sum + r.sizeBytes, 0),
  );

  return (
    <>
      <Header
        title="Library"
        sub={`${app.library().length} models · counted once each`}
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
          <CategoryMenu categories={categories()} />
          <Show when={app.usage().size > 0 && !app.nothingSearched()}>
            <button
              class="chip"
              classList={{ on: app.lib.unusedOnly }}
              aria-pressed={app.lib.unusedOnly}
              onClick={() => app.setLib("unusedOnly", !app.lib.unusedOnly)}
            >
              Not used &middot; {app.unusedCount()}
            </button>
          </Show>
          <span style={{ flex: 1 }} />
          <span class="count">
            {rows().length} of {app.libraryTotal()} &middot; {fmt(shownBytes())}
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

            <Show when={app.lib.unusedOnly && app.usageMethod()}>
              {(method) => (
                <div class="lib-method" role="note">
                  {method()} A workflow you never saved lives in the browser,
                  where ComfyVault cannot see it, so this list is not a list of
                  models that are safe to delete.
                </div>
              )}
            </Show>
            <For each={rows()}>
              {(row) => {
                const answer = () => app.usage().get(row.name);
                // No dot at all when nothing was searched: an empty dot would
                // read as "checked, and nothing uses it".
                const used = () =>
                  answer() === undefined || app.nothingSearched()
                    ? undefined
                    : answer()!.used;
                return (
                  <button
                    class="lrow"
                    classList={{ on: row.sha256 === app.lib.selected }}
                    title={row.name}
                    onClick={() =>
                      app.setLib({ selected: row.sha256, drawerOpen: true })
                    }
                  >
                    <span
                      class="dot"
                      classList={{ used: used() === true, unused: used() === false }}
                    />
                    <span class="ln">{mid(row.name, narrow() ? 34 : 72)}</span>
                    <Show when={!narrow()}>
                      <span class="lf">{row.category}</span>
                    </Show>
                    <span class="lz">{fmt(row.sizeBytes)}</span>
                    <Show when={!narrow()}>
                      <span
                        class="lk"
                        classList={{ none: row.occurrenceCount === 0 }}
                      >
                        {row.occurrenceCount}
                      </span>
                    </Show>
                    <span class="go">
                      <Icon name="arrow" size={12} />
                    </span>
                  </button>
                );
              }}
            </For>

            <Show when={rows().length === 0}>
              <div class="lib-empty">
                <div class="lbl">Nothing matches</div>
                <div class="note">
                  No model here is named{" "}
                  {app.lib.query ? `“${app.lib.query}”` : "that"}
                  {app.lib.category !== "all" ? ` inside ${app.lib.category}` : ""}
                  {app.lib.unusedOnly ? " and unused" : ""}.
                </div>
                <div class="acts">
                  <button
                    class="btn sm"
                    onClick={() =>
                      app.setLib({
                        query: "",
                        category: "all",
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
            <Drawer row={selected()!} />
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

function CategoryMenu(props: { categories: readonly string[] }) {
  const app = useApp();
  const items = createMemo(() => [
    { key: "all", label: "All folders", count: app.library().length },
    ...props.categories.map((category) => ({
      key: category,
      label: category,
      count: app.library().filter((r) => r.category === category).length,
    })),
  ]);

  return (
    <span class="menu-wrap">
      <button
        class="sel"
        classList={{ on: app.lib.category !== "all" }}
        aria-haspopup="menu"
        aria-expanded={app.categoryMenuOpen()}
        onClick={(e) => {
          e.stopPropagation();
          app.actions.setCategoryMenuOpen(!app.categoryMenuOpen());
        }}
      >
        <Icon name="folder" size={12} />
        <span>{app.lib.category === "all" ? "All folders" : app.lib.category}</span>
        <Icon name="chev" size={11} />
      </button>
      <Show when={app.categoryMenuOpen()}>
        <div class="menu" role="menu">
          <For each={items()}>
            {(item) => (
              <button
                class="mitem"
                classList={{ on: app.lib.category === item.key }}
                role="menuitemradio"
                aria-checked={app.lib.category === item.key}
                onClick={() => {
                  app.setLib("category", item.key);
                  app.actions.setCategoryMenuOpen(false);
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

function Drawer(props: { row: ContentRow }) {
  const app = useApp();
  // Nothing was checked is not an answer, so the header says nothing either.
  const used = () => {
    const answer = app.usage().get(props.row.name);
    if (!answer || nothingWasSearched(answer)) return undefined;
    return answer.used;
  };
  return (
    <div class="drawer">
      <div class="dhead">
        <div class="dt">
          <div class="det-name" title={props.row.name}>
            <Wrap text={props.row.name} />
          </div>
          <div class="det-meta">
            {props.row.category} &nbsp;&middot;&nbsp; {fmt(props.row.sizeBytes)}
            <Show when={used() !== undefined}>
              {" "}
              &nbsp;&middot;&nbsp;
              <Show
                when={used()}
                fallback={<span style={{ color: "var(--t-muted)" }}>Not used</span>}
              >
                <span class="grn">In use</span>
              </Show>
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
        <DrawerBody row={props.row} />
      </div>
    </div>
  );
}

function DrawerBody(props: { row: ContentRow }) {
  const app = useApp();
  const row = () => props.row;
  const answer = () => app.usage().get(row().name);

  /**
   * Where the vault keeps it, or will. The plan is the authority once there is
   * one, because it carries the renamed form when two files want one name.
   */
  const vaultPath = () => {
    const group = app.plan()?.groups.find((g) => g.sha256 === row().sha256);
    const rel = group?.vaultRelPath ?? `${row().category}/${row().name}`;
    return `${app.vault()?.root ?? ""}\\${rel.replace(/\//g, "\\")}`;
  };

  // The list gives one row per content. The paths behind it are fetched for the
  // one row the person opened, rather than for every row nobody looked at.
  const [places] = createResource(
    () => row().sha256,
    async (sha256) => {
      const links = row().inVault
        ? await app.engine.listLinks({ sha256 })
        : [];
      return placesOf(
        sha256,
        app.plan(),
        links,
        new Map(app.installs().map((i) => [i.id, i.label])),
      );
    },
  );

  const deleteFromVault = () => {
    openConfirm(app, {
      title: "Delete a vault file",
      cta: "Delete it",
      body: [
        [
          { text: row().name, emph: true },
          { text: " is deleted from the vault and " },
          { text: fmt(row().sizeBytes), emph: true },
          {
            text: " comes back. Nothing points at it today. This cannot be undone: the bytes are gone.",
          },
        ],
      ],
      action: async () => {
        await app.engine.deleteVaultFile(row().sha256, row().sha256);
        app.setLib({ selected: null, drawerOpen: false });
      },
    });
  };

  return (
    <>
      <div class="det-acts">
        <Show when={row().inVault}>
          <button class="btn sm" onClick={() => void openLinkPicker(app, row().sha256)}>
            <Icon name="plus" size={11} />
            Link into an instance
          </button>
        </Show>
        <Show when={row().inVault && row().occurrenceCount === 0}>
          <button class="btn sm dng" onClick={deleteFromVault}>
            <Icon name="trash" size={11} />
            Delete
          </button>
        </Show>
      </div>

      <div class="sec" style={{ "margin-top": "14px" }}>
        <span class="t">Where it reaches</span>
        <span class="n">{row().occurrenceCount || "none"}</span>
      </div>
      <Show
        when={(places() ?? []).length > 0}
        fallback={
          <div class="note">
            Nothing points at this file. It is in the vault because an install that
            used it is no longer registered. Link it into an install, or delete it
            and get {fmt(row().sizeBytes)} back.
          </div>
        }
      >
        <div class="reach">
          <For each={places() ?? []}>
            {(place) => (
              <div class="r">
                <div class="top">
                  <Icon
                    name={
                      place.blocked
                        ? "file"
                        : place.kind === "isLink"
                          ? "link"
                          : place.kind === "source"
                            ? "vault"
                            : "link"
                    }
                    size={12}
                  />
                  <span>{place.installLabel || "outside an install"}</span>
                  <span class="sp" />
                  <Show
                    when={place.blocked}
                    fallback={
                      <span
                        class="pill"
                        classList={{
                          link: place.kind === "isLink",
                          pend: place.kind !== "isLink",
                        }}
                      >
                        {place.kind === "isLink"
                          ? "link"
                          : place.kind === "source"
                            ? "becomes the vault copy"
                            : "becomes a link"}
                      </span>
                    }
                  >
                    {(blocked) => (
                      <span class="pill bad">{blockedShort(blocked().reason)}</span>
                    )}
                  </Show>
                </div>
                <div class="pp">
                  <Wrap text={place.absPath} />
                </div>
                <Show when={place.name !== row().name}>
                  <div class="pp alt">filename here: {place.name}</div>
                </Show>
                <Show when={place.blocked}>
                  {(blocked) => <div class="pp">{blockedWhy(blocked())}</div>}
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
          <Wrap text={vaultPath()} />
        </span>
      </div>
      <div class="kv">
        <span class="k w96">Size</span>
        <span class="v">{fmtExactMB(row().sizeBytes)}</span>
      </div>
      <div class="kv">
        <span class="k w96">SHA-256</span>
        <span class="v faint" style={{ "font-size": "10px" }}>
          {shortHash(row().sha256)}
        </span>
      </div>
      <div class="kv st">
        <span class="k">Added to the vault</span>
        <span class="v">
          {row().addedAt
            ? dayMonth(row().addedAt!)
            : "not yet, this plan has not been applied"}
        </span>
      </div>

      <div class="sec secgap plain">
        <span class="t">Used by a workflow</span>
      </div>
      <Show
        when={answer()}
        fallback={
          <div class="note">
            ComfyVault could not read the saved workflow files, so it cannot say
            whether anything names this model.
          </div>
        }
      >
        {(result) => (
          <>
            <Show
              when={result().used}
              fallback={
                <Show
                  when={!nothingWasSearched(result())}
                  fallback={
                    <div class="note">
                      Nothing was checked for this model.
                    </div>
                  }
                >
                  <div class="note">
                    This filename appears in{" "}
                    <span class="emph">no saved workflow file</span>.
                  </div>
                </Show>
              }
            >
              <div class="note">
                This filename appears in{" "}
                <span class="emph">
                  {result().matches.length} saved workflow{" "}
                  {result().matches.length === 1 ? "file" : "files"}
                </span>
                .
              </div>
              <For each={result().matches.slice(0, 4)}>
                {(match) => (
                  <div class="kv namerow">
                    <span class="v faint" style={{ "font-size": "10px" }}>
                      {match.installLabel} &middot; {match.workflowName}
                    </span>
                  </div>
                )}
              </For>
            </Show>
            <div class="note up">{result().method}</div>
            <Show when={!nothingWasSearched(result())}>
              <div class="note">
                A workflow you never saved lives in the browser, where ComfyVault
                cannot see it, so this is not proof that nothing uses the model.
              </div>
            </Show>
          </>
        )}
      </Show>

      <Show when={row().aliases.length > 0}>
        <div class="sec secgap plain">
          <span class="t">Other names for these bytes</span>
          <span class="n">{row().aliases.length + 1}</span>
        </div>
        <For each={[row().name, ...row().aliases]}>
          {(name) => (
            <div class="kv namerow">
              <span class="v">
                {name}
                <Show when={name === row().name}>
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

      <Show when={row().installIds.length > 0}>
        <ThumbnailNoteForModel installIds={row().installIds} />
      </Show>

      <div class="sec secgap plain">
        <span class="t">Civitai</span>
      </div>
      <Show
        when={row().metadata?.found ? row().metadata : null}
        fallback={
          <Show
            when={app.appState()?.settings.metadataLookupsEnabled}
            fallback={
              <div class="note">
                Civitai lookup is off, so nothing was asked about this file.
                ComfyVault works the same either way. Turn it on in Settings.
              </div>
            }
          >
            <div class="note">
              Civitai has no file with this fingerprint. That is normal: official
              releases and anything you built or renamed yourself will never match.
              ComfyVault works the same either way.
            </div>
          </Show>
        }
      >
        {(meta) => (
          <>
            <div class="kv">
              <span class="k w96">Name</span>
              <span class="v">
                <b>{meta().modelName}</b>
              </span>
            </div>
            <div class="kv">
              <span class="k w96">Version</span>
              <span class="v">{meta().versionName}</span>
            </div>
            <div class="kv">
              <span class="k w96">Type</span>
              <span class="v">
                {meta().modelType}
                <Show when={meta().baseModel}> &middot; {meta().baseModel}</Show>
              </span>
            </div>
            <Show when={meta().triggerWords.length > 0}>
              <div class="kv">
                <span class="k w96">Trigger words</span>
                <span class="v">{meta().triggerWords.join(", ")}</span>
              </div>
            </Show>
            <Show when={meta().pageUrl}>
              {(url) => (
                <div class="det-acts" style={{ "margin-top": "9px" }}>
                  <button
                    class="btn sm"
                    onClick={() => void app.engine.openExternal(url())}
                  >
                    <Icon name="external" size={11} />
                    Open on Civitai
                  </button>
                </div>
              )}
            </Show>
            <Show when={meta().ambiguous}>
              <div class="note up">
                More than one upload on Civitai has these exact bytes. This is the
                earliest one.
              </div>
            </Show>
            <div class="note up">
              Returned by Civitai for this file&rsquo;s fingerprint. ComfyVault
              stores nothing else about it.
            </div>
          </>
        )}
      </Show>
    </>
  );
}
