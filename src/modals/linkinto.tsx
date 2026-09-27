import { For, Show, createMemo } from "solid-js";

import { Icon } from "~/components/Icon";
import { Wrap } from "~/components/Wrap";
import { folderNameError } from "~/domain/foldername";
import { joinPath } from "~/domain/format";
import { installName } from "~/domain/installname";
import { current, inside, load, open, patch, same } from "~/modals/linkfolder";
import { messageOf, useApp } from "~/state/store";

/**
 * The Library's "Link into an install": the install, then the folder.
 *
 * The folders are the ones ComfyUI reads for the model's kind in that install,
 * with the one used last time first. A new folder is only drawn here. It is
 * made when the link is.
 */
export function LinkIntoView() {
  const app = useApp();
  const m = () => {
    const x = current(app);
    return x && x.sha256 !== null ? x : null;
  };
  const install = () => app.installs().find((i) => i.id === m()?.installId);
  const nameOf = () => {
    const inst = install();
    return inst ? installName(inst, app.installs()) : "";
  };

  /** Installs that already link this model: nothing to add there. */
  const linkedIn = createMemo(() => {
    const sha = m()?.sha256;
    const file = app.vaultFiles().find((f) => f.sha256 === sha);
    return new Set(file?.links.map((l) => l.installId) ?? []);
  });

  /**
   * The folder used last time, then the others ComfyUI reads. Each opens to
   * show the folders inside it, and a new one sits inside where it was made.
   */
  const rows = createMemo(() => {
    const x = m();
    const inst = install();
    if (!x || !inst) return [];
    const roots = x.folders[""] ?? [];
    const top: { path: string; last: boolean }[] = [];
    if (x.lastUsed) top.push({ path: x.lastUsed, last: true });
    for (const root of roots) {
      if (!top.some((r) => same(r.path, root.path))) top.push({ path: root.path, last: false });
    }
    const known = (path: string) =>
      Object.values(x.folders)
        .flat()
        .find((f) => same(f.path, path));
    const out: { path: string; label: string; depth: number; last: boolean; canOpen: boolean }[] = [];
    // As the approved design writes them: each folder from the install's root.
    const labelOf = (path: string) => (inside(path, inst.root) ? path.slice(inst.root.length + 1) : path);
    const walk = (path: string, depth: number, last: boolean, isNew: boolean) => {
      const added = x.added.filter((a) => same(a.slice(0, a.lastIndexOf("\\")), path));
      const children = x.folders[path];
      out.push({
        path,
        label: labelOf(path),
        depth,
        last,
        // Not read yet: it may hold folders, so it can be opened to see.
        canOpen: added.length > 0 || (children ? children.length > 0 : !isNew && (known(path)?.hasSubfolders ?? true)),
      });
      if (!x.expanded.includes(path)) return;
      for (const child of children ?? []) walk(child.path, depth + 1, false, false);
      for (const a of added) walk(a, depth + 1, false, true);
    };
    for (const t of top) walk(t.path, 0, t.last, false);
    return out;
  });

  const toggle = (path: string) => {
    const x = m();
    if (!x) return;
    if (x.expanded.includes(path)) {
      patch(app, (y) => (y.expanded = y.expanded.filter((p) => p !== path)));
    } else void open(app, path);
  };

  const choose = (id: string) => {
    patch(app, (y) => (y.installId = id));
    void load(app, "");
  };

  const makeFolder = () => {
    const x = m();
    if (!x?.naming || !x.selected) return;
    const parent = x.selected;
    const siblings = [
      ...(x.folders[parent] ?? []).map((f) => f.path),
      ...x.added.filter((a) => same(a.slice(0, a.lastIndexOf("\\")), parent)),
    ];
    const problem = folderNameError(x.naming.draft, parent, siblings);
    if (problem) {
      patch(app, (y) => y.naming && (y.naming.error = problem));
      return;
    }
    const path = joinPath(parent, x.naming.draft.trim());
    patch(app, (y) => {
      y.added.push(path);
      if (!y.expanded.includes(parent)) y.expanded.push(parent);
      y.selected = path;
      y.naming = null;
      y.error = null;
    });
  };

  const linkHere = async () => {
    const x = m();
    if (!x?.selected || !x.installId || x.working) return;
    const who = nameOf();
    patch(app, (y) => {
      y.working = true;
      y.error = null;
    });
    try {
      await app.engine.createLink({
        installId: x.installId,
        sha256: x.sha256!,
        dir: x.selected,
        createDir: true,
      });
      app.setModal(null);
      app.actions.showToast(`Linked in ${who}.`);
      await app.actions.refresh();
    } catch (error) {
      patch(app, (y) => {
        y.working = false;
        y.error = messageOf(error);
      });
    }
  };

  return (
    <Show when={m()}>
      {(x) => (
        <div
          class="veil"
          onClick={(e) => {
            if (e.target === e.currentTarget && !x().working) app.setModal(null);
          }}
        >
          <div
            class="modal"
            role="dialog"
            aria-modal="true"
            aria-label={x().installId ? `${nameOf()} · Choose a folder` : "Link into an install"}
          >
            <div class="mh">
              <Icon name="folder" size={14} />
              <h2>{x().installId ? `${nameOf()} · Choose a folder` : "Link into an install"}</h2>
            </div>
            <div class="mb">
              <Show
                when={x().installId}
                fallback={
                  <div class="tree">
                    <For each={app.installs()}>
                      {(inst) => (
                        <button
                          class="tnode"
                          disabled={linkedIn().has(inst.id)}
                          title={inst.root}
                          onClick={() => choose(inst.id)}
                        >
                          <Icon name="folder" size={12} />
                          <span>{installName(inst, app.installs())}</span>
                          <Show when={linkedIn().has(inst.id)}>
                            <span class="hint">Already linked</span>
                          </Show>
                        </button>
                      )}
                    </For>
                  </div>
                }
              >
                <div class="tree" role="tree">
                  <For each={rows()}>
                    {(row) => (
                      <div class="trow" style={{ "padding-left": `${row.depth * 16}px` }}>
                        <Show when={row.canOpen} fallback={<span class="twist leaf" />}>
                          <button
                            class="twist"
                            classList={{
                              open: x().expanded.includes(row.path),
                              shut: !x().expanded.includes(row.path),
                              busy: x().loading.includes(row.path),
                            }}
                            aria-expanded={x().expanded.includes(row.path)}
                            aria-label={`${x().expanded.includes(row.path) ? "Close" : "Open"} ${row.label}`}
                            onClick={() => toggle(row.path)}
                          >
                            <Icon name={x().loading.includes(row.path) ? "refresh" : "chev"} size={11} />
                          </button>
                        </Show>
                        <button
                          class="tnode"
                          classList={{ on: x().selected !== null && same(x().selected!, row.path) }}
                          role="treeitem"
                          aria-selected={x().selected !== null && same(x().selected!, row.path)}
                          title={row.path}
                          onClick={() =>
                            patch(app, (y) => {
                              y.selected = row.path;
                              y.error = null;
                            })
                          }
                        >
                          <Icon name="folder" size={12} />
                          <span>{row.label}</span>
                          <Show when={row.last}>
                            <span class="hint">Last used</span>
                          </Show>
                        </button>
                      </div>
                    )}
                  </For>
                </div>
                <div style={{ "margin-top": "8px" }}>
                  <Show
                    when={x().naming}
                    fallback={
                      <button
                        class="btn sm"
                        disabled={!x().selected}
                        onClick={() => patch(app, (y) => (y.naming = { draft: "", error: null }))}
                      >
                        <Icon name="plus" size={11} />
                        New folder
                      </button>
                    }
                  >
                    {(naming) => (
                      <>
                        <div class="newf">
                          <label class="field">
                            <Icon name="folder" size={12} />
                            <input
                              value={naming().draft}
                              aria-label="Name of the new folder"
                              ref={(el) => queueMicrotask(() => el.focus())}
                              onInput={(e) => {
                                const value = e.currentTarget.value;
                                patch(app, (y) => {
                                  if (y.naming) {
                                    y.naming.draft = value;
                                    y.naming.error = null;
                                  }
                                });
                              }}
                              onKeyDown={(e) => {
                                if (e.key === "Enter") {
                                  e.preventDefault();
                                  makeFolder();
                                }
                                if (e.key === "Escape") {
                                  e.preventDefault();
                                  e.stopPropagation();
                                  patch(app, (y) => (y.naming = null));
                                }
                              }}
                            />
                          </label>
                          <button class="btn sm pri" onClick={makeFolder}>
                            Create
                          </button>
                        </div>
                        <Show when={naming().error}>
                          {(why) => (
                            <div class="tnew-err" role="alert" style={{ "padding-left": "0" }}>
                              {why()}
                            </div>
                          )}
                        </Show>
                      </>
                    )}
                  </Show>
                </div>
              </Show>
              <Show when={x().error}>
                {(why) => (
                  <div class="verdict no" role="alert">
                    <p>{why()}</p>
                  </div>
                )}
              </Show>
            </div>
            <div class="mf">
              <Show when={x().installId} fallback={<span class="sp" />}>
                <span class="res">
                  <Show when={x().selected}>
                    {(dir) => <Wrap text={joinPath(dir(), x().fileName)} />}
                  </Show>
                </span>
              </Show>
              <button class="btn" disabled={x().working} onClick={() => app.setModal(null)}>
                Cancel
              </button>
              <Show when={x().installId}>
                <button
                  class="btn pri"
                  disabled={!x().selected || x().working || app.linkBusy() !== null}
                  onClick={() => void linkHere()}
                >
                  {x().working ? "Working…" : (app.linkBusy() ?? "Link here")}
                </button>
              </Show>
            </div>
          </div>
        </div>
      )}
    </Show>
  );
}
