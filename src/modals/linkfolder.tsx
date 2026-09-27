import { For, Show, createMemo } from "solid-js";

import { Icon } from "~/components/Icon";
import { folderNameError } from "~/domain/foldername";
import { joinPath } from "~/domain/format";
import { installName } from "~/domain/installname";
import { fileNameOf } from "~/domain/view";
import { messageOf, useApp, type AppStore, type LinkFolderModal } from "~/state/store";
import type { Install, LinkFolder } from "~/ipc/contract";

/**
 * The folder a new link goes in.
 *
 * Only the folders ComfyUI reads for the model's kind in that install are
 * offered, and the folders inside them. A checkpoint linked into loras would
 * not show in ComfyUI's checkpoint list, so the tree never goes above them. A
 * new folder is only drawn here. It is made when the link is.
 */

/** The chooser for one install, from the Download card. */
export function openLinkFolderChooser(
  app: AppStore,
  options: {
    installId: string;
    category: string;
    fileName: string;
    current: string | null;
    onUse: (dir: string) => void;
  },
): void {
  app.setModal({
    kind: "linkFolder",
    installId: options.installId,
    category: options.category,
    fileName: options.fileName,
    sha256: null,
    selected: options.current,
    folders: {},
    expanded: [],
    loading: [],
    added: [],
    naming: null,
    error: null,
    working: false,
    onUse: options.onUse,
  });
  void loadRoots(app, options.current);
}

/** The Library's "Link into an install": first the install, then the folder. */
export function openLibraryLinkChooser(
  app: AppStore,
  options: { sha256: string; category: string; fileName: string },
): void {
  app.setModal({
    kind: "linkFolder",
    installId: null,
    category: options.category,
    fileName: options.fileName,
    sha256: options.sha256,
    selected: null,
    folders: {},
    expanded: [],
    loading: [],
    added: [],
    naming: null,
    error: null,
    working: false,
    onUse: null,
  });
}

const current = (app: AppStore): LinkFolderModal | null => {
  const m = app.modal();
  return m && m.kind === "linkFolder" ? m : null;
};

const patch = (app: AppStore, fn: (m: LinkFolderModal) => void) =>
  app.patchModal((m) => {
    if (m.kind === "linkFolder") fn(m);
  });

const same = (a: string, b: string) => a.toLowerCase() === b.toLowerCase();
const inside = (path: string, parent: string) =>
  path.toLowerCase().startsWith(`${parent.toLowerCase()}\\`);

/** The folders under one folder, "" for the roots. */
async function load(app: AppStore, dir: string): Promise<void> {
  const m = current(app);
  if (!m || !m.installId || m.folders[dir] || m.loading.includes(dir)) return;
  patch(app, (x) => void x.loading.push(dir));
  try {
    const list = await app.engine.listLinkFolders({
      installId: m.installId,
      category: m.category,
      ...(dir === "" ? {} : { dir }),
    });
    patch(app, (x) => {
      x.folders[dir] = list.folders;
      x.loading = x.loading.filter((d) => d !== dir);
      // The folder remembered for this install and kind, where the chooser opens.
      if (dir === "" && !x.selected) x.selected = list.defaultDir;
    });
  } catch (error) {
    patch(app, (x) => {
      x.loading = x.loading.filter((d) => d !== dir);
      x.error = messageOf(error);
    });
  }
}

/** Load the roots, and open the way down to the folder already chosen. */
async function loadRoots(app: AppStore, selected: string | null): Promise<void> {
  await load(app, "");
  const m = current(app);
  if (!m) return;
  const roots = m.folders[""] ?? [];
  if (!m.selected) patch(app, (x) => (x.selected = roots[0]?.path ?? null));
  const target = selected ?? current(app)?.selected ?? null;
  for (const root of roots) {
    if (!root.exists || !root.hasSubfolders) continue;
    // The first root, and the one holding the choice, open at once.
    if (root !== roots[0] && !(target && inside(target, root.path))) continue;
    await open(app, root.path);
    // Every folder between the root and the choice opens too.
    if (target && inside(target, root.path)) {
      let path = root.path;
      for (const part of target.slice(root.path.length + 1).split("\\").slice(0, -1)) {
        path = `${path}\\${part}`;
        await open(app, path);
      }
    }
  }
}

async function open(app: AppStore, path: string): Promise<void> {
  patch(app, (x) => {
    if (!x.expanded.includes(path)) x.expanded.push(path);
  });
  await load(app, path);
}

interface Row {
  path: string;
  label: string;
  depth: number;
  hint: string | null;
  isNew: boolean;
  canOpen: boolean;
}

/** What a root is, said in words. */
function rootHint(root: LinkFolder, install: Install, category: string): string {
  if (root.origin === "extraPath") return "from this install's extra_model_paths.yaml";
  if (root.origin === "outputDir") return "an output folder ComfyUI also reads";
  if (same(root.path, `${install.modelsDir}\\${category}`)) return "the usual place";
  return "an older folder name ComfyUI still reads";
}

export function LinkFolderView() {
  const app = useApp();
  const m = () => current(app);
  const install = () => app.installs().find((i) => i.id === m()?.installId);

  const rows = createMemo<Row[]>(() => {
    const x = m();
    const inst = install();
    if (!x || !inst) return [];
    const out: Row[] = [];
    const walk = (folder: LinkFolder | { path: string; hasSubfolders: boolean; exists: boolean }, depth: number, label: string, hint: string | null, isNew: boolean) => {
      const added = x.added.filter((a) => same(a.slice(0, a.lastIndexOf("\\")), folder.path));
      out.push({
        path: folder.path,
        label,
        depth,
        hint,
        isNew,
        canOpen: folder.hasSubfolders || added.length > 0,
      });
      if (!x.expanded.includes(folder.path)) return;
      for (const child of x.folders[folder.path] ?? []) walk(child, depth + 1, child.name, null, false);
      for (const a of added) {
        walk({ path: a, hasSubfolders: false, exists: false }, depth + 1, fileNameOf(a), null, true);
      }
    };
    for (const root of x.folders[""] ?? []) {
      const label = inside(root.path, inst.root) ? root.path.slice(inst.root.length + 1) : root.path;
      walk(root, 0, label, rootHint(root, inst, x.category), false);
    }
    return out;
  });

  const toggle = (row: Row) => {
    const x = m();
    if (!x) return;
    if (x.expanded.includes(row.path)) {
      patch(app, (y) => (y.expanded = y.expanded.filter((p) => p !== row.path)));
    } else void open(app, row.path);
  };

  const selectedName = () => fileNameOf(m()?.selected ?? "");

  const siblingsOf = (parent: string) => [
    ...(m()?.folders[parent] ?? []).map((f) => f.path),
    ...(m()?.added ?? []).filter((a) => same(a.slice(0, a.lastIndexOf("\\")), parent)),
  ];

  const makeFolder = () => {
    const x = m();
    if (!x?.naming || !x.selected) return;
    const parent = x.selected;
    const problem = folderNameError(x.naming.draft, parent, siblingsOf(parent));
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
    });
  };

  const use = async () => {
    const x = m();
    if (!x?.selected || !x.installId || x.working) return;
    if (x.onUse) {
      x.onUse(x.selected);
      app.setModal(null);
      return;
    }
    // From the Library, the chooser makes the link itself.
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
      const name = install() ? installName(install()!, app.installs()) : x.installId;
      app.setModal(null);
      app.actions.showToast(`Linked ${x.fileName} in ${name}.`);
      await app.actions.refresh();
    } catch (error) {
      patch(app, (y) => {
        y.working = false;
        y.error = messageOf(error);
      });
    }
  };

  const chooseInstall = (id: string) => {
    patch(app, (y) => (y.installId = id));
    void loadRoots(app, null);
  };

  /** Installs that already link this model: nothing to add there. */
  const linkedIn = createMemo(() => {
    const sha = m()?.sha256;
    if (!sha) return new Set<string>();
    const file = app.vaultFiles().find((f) => f.sha256 === sha);
    return new Set(file?.links.map((l) => l.installId) ?? []);
  });

  return (
    <Show when={m()}>
      {(x) => (
        <div
          class="veil"
          onClick={(e) => {
            if (e.target === e.currentTarget && !x().working) app.setModal(null);
          }}
        >
          <div class="modal" role="dialog" aria-modal="true" aria-label="Where the link goes">
            <div class="mh">
              <Icon name="folder" size={14} />
              <h2>
                <Show when={install()} fallback={<>Link {x().fileName} into an install</>}>
                  {(inst) => <>Where the link goes in {installName(inst(), app.installs())}</>}
                </Show>
              </h2>
            </div>
            <div class="mb">
              <Show
                when={x().installId}
                fallback={
                  <>
                    <p class="mnote">
                      Pick the install the link goes in. Then pick the folder inside it.
                    </p>
                    <div class="tree">
                      <For each={app.installs()}>
                        {(inst) => (
                          <button
                            class="tnode"
                            disabled={linkedIn().has(inst.id)}
                            title={inst.root}
                            onClick={() => chooseInstall(inst.id)}
                          >
                            <Icon name="folder" size={12} />
                            <span>{installName(inst, app.installs())}</span>
                            <span class="hint">
                              {linkedIn().has(inst.id) ? "already has this link" : inst.root}
                            </span>
                          </button>
                        )}
                      </For>
                    </div>
                  </>
                }
              >
                <p class="mnote">
                  ComfyUI finds {x().category} in these folders and in every folder inside
                  them. Pick the folder you keep this kind of model in, or make a new one.
                </p>
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
                            aria-label={`${x().expanded.includes(row.path) ? "Collapse" : "Open"} ${row.label}`}
                            onClick={() => toggle(row)}
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
                          onClick={() => patch(app, (y) => (y.selected = row.path))}
                        >
                          <Icon name="folder" size={12} />
                          <span>{row.label}</span>
                          <Show when={row.isNew}>
                            <span class="tnew-tag">new</span>
                          </Show>
                          <Show when={row.hint}>{(hint) => <span class="hint">{hint()}</span>}</Show>
                        </button>
                      </div>
                    )}
                  </For>
                </div>
                <div style={{ "margin-top": "8px" }}>
                  <Show
                    when={x().naming}
                    fallback={
                      <Show when={x().selected}>
                        <button
                          class="btn sm"
                          onClick={() => patch(app, (y) => (y.naming = { draft: "", error: null }))}
                        >
                          <Icon name="plus" size={11} />
                          New folder in {selectedName()}
                        </button>
                      </Show>
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
                            Make it
                          </button>
                          <button class="btn sm" onClick={() => patch(app, (y) => (y.naming = null))}>
                            Cancel
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
                    <h4>
                      <Icon name="x" size={12} />
                      That did not happen
                    </h4>
                    <p>{why()}</p>
                  </div>
                )}
              </Show>
            </div>
            <div class="mf">
              <span class="res">
                <Show when={x().installId && x().selected}>
                  <span class="faint">The link will be</span>{" "}
                  {joinPath(x().selected!, x().fileName)}
                </Show>
              </span>
              <button class="btn" disabled={x().working} onClick={() => app.setModal(null)}>
                Cancel
              </button>
              <button
                class="btn pri"
                disabled={!x().installId || !x().selected || x().working}
                onClick={() => void use()}
              >
                {x().working ? "Working…" : "Use this folder"}
              </button>
            </div>
          </div>
        </div>
      )}
    </Show>
  );
}
