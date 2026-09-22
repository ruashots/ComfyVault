import { For, Show, createMemo, createSignal, type JSX } from "solid-js";

import { Icon } from "~/components/Icon";
import { fmt, joinPath, leafOf } from "~/domain/format";
import { folderNameError } from "~/domain/foldername";
import {
  messageOf,
  useApp,
  type AppStore,
  type PickerPurpose,
  type TreeNode,
} from "~/state/store";
import type { DirectoryEntry, InstallCandidate } from "~/ipc/contract";

// ── opening one ─────────────────────────────────────────────────────────────

async function openPicker(
  app: AppStore,
  purpose: PickerPurpose,
  extra: { sha256?: string; replacing?: string } = {},
): Promise<void> {
  const roots = await app.engine.listDirectory(null);
  app.setModal({
    kind: "picker",
    purpose,
    nodes: roots.entries.map((entry) => toNode(entry, 0)),
    expanded: [],
    loading: [],
    picked: null,
    candidate: null,
    checking: false,
    newFolder: null,
    sha256: extra.sha256 ?? null,
    replacing: extra.replacing ?? null,
  });
}

export const openInstallPicker = (app: AppStore) => openPicker(app, "install");
export const openVaultPicker = (app: AppStore) => openPicker(app, "vault");
export const openLinkPicker = (app: AppStore, sha256: string) =>
  openPicker(app, "link", { sha256 });

const DRIVE_ROOT = /^[A-Za-z]:\\$/;

function toNode(entry: DirectoryEntry, depth: number): TreeNode {
  return {
    ...entry,
    depth,
    isNew: false,
    isDrive: DRIVE_ROOT.test(entry.path),
    hasChildren: null,
    refusal: null,
  };
}

// ── the modal ───────────────────────────────────────────────────────────────

export function PickerModalView() {
  const app = useApp();
  const modal = () => {
    const current = app.modal();
    return current && current.kind === "picker" ? current : null;
  };
  const [confirming, setConfirming] = createSignal(false);

  const title = () => {
    switch (modal()?.purpose) {
      case "vault":
        return "Choose the vault folder";
      case "link":
        return "Choose where the link goes";
      default:
        return "Choose a ComfyUI install folder";
    }
  };
  const cta = () => {
    switch (modal()?.purpose) {
      case "vault":
        return "Use this folder";
      case "link":
        return "Put the link here";
      default:
        return "Add this install";
    }
  };

  /** Everything under a folder, for the prefix test. "C:\" has no extra slash. */
  const insideOf = (path: string) => (path.endsWith("\\") ? path : `${path}\\`);

  const collapse = (node: TreeNode) => {
    const inside = insideOf(node.path).toLowerCase();
    app.patchModal((m) => {
      if (m.kind !== "picker") return;
      m.expanded = m.expanded.filter((p) => p !== node.path);
      m.nodes = m.nodes.filter(
        (n) => n.path === node.path || !n.path.toLowerCase().startsWith(inside),
      );
    });
  };

  const expand = async (node: TreeNode) => {
    const current = modal();
    if (!current || current.expanded.includes(node.path)) return;
    app.patchModal((m) => {
      if (m.kind === "picker") m.loading = [...m.loading, node.path];
    });
    let entries: DirectoryEntry[];
    try {
      entries = (await app.engine.listDirectory(node.path)).entries;
    } catch (failure) {
      // A folder that cannot be read is a refusal, not an empty folder. Say so
      // on the row rather than leaving it looking like there is nothing inside.
      app.patchModal((m) => {
        if (m.kind !== "picker") return;
        m.loading = m.loading.filter((p) => p !== node.path);
        const at = m.nodes.findIndex((n) => n.path === node.path);
        if (at < 0) return;
        m.nodes[at]!.refusal = messageOf(failure);
        m.nodes[at]!.hasChildren = false;
      });
      return;
    }
    app.patchModal((m) => {
      if (m.kind !== "picker") return;
      m.loading = m.loading.filter((p) => p !== node.path);
      m.expanded = [...m.expanded, node.path];
      const at = m.nodes.findIndex((n) => n.path === node.path);
      if (at < 0) return;
      m.nodes[at]!.hasChildren = entries.length > 0;
      m.nodes[at]!.refusal = null;
      m.nodes = [
        ...m.nodes.slice(0, at + 1),
        ...entries.map((child) => toNode(child, node.depth + 1)),
        ...m.nodes.slice(at + 1),
      ];
    });
  };

  const toggle = async (node: TreeNode) => {
    const current = modal();
    if (!current) return;
    if (current.expanded.includes(node.path)) collapse(node);
    else await expand(node);
  };

  /**
   * Picking a folder asks the engine what it is. Only the install picker has a
   * judgement to ask for; the other two are judged here, against facts the
   * interface already holds.
   */
  const pick = async (node: TreeNode) => {
    const current = modal();
    if (!current) return;
    app.patchModal((m) => {
      if (m.kind !== "picker") return;
      m.picked = node.path;
      m.candidate = null;
      m.checking = current.purpose === "install";
      m.newFolder = null;
    });
    if (current.purpose !== "install") return;
    try {
      const candidate = await app.engine.validateInstallPath(node.path);
      app.patchModal((m) => {
        if (m.kind !== "picker" || m.picked !== node.path) return;
        m.candidate = candidate;
        m.checking = false;
      });
    } catch (error) {
      app.patchModal((m) => {
        if (m.kind !== "picker" || m.picked !== node.path) return;
        m.checking = false;
        m.candidate = {
          valid: false,
          root: null,
          nestedDepth: 0,
          markersFound: [],
          markersMissing: [],
          contentCheckPassed: false,
          otherCandidates: [],
          version: null,
          versionSource: null,
          modelsDir: null,
          modelsDirExists: false,
          extraPathsFile: null,
          extraPaths: [],
          extraPathsError: null,
          outputModelDirs: [],
          reason: messageOf(error),
        };
      });
    }
  };

  const siblingsOf = (parent: string) => {
    const inside = insideOf(parent).toLowerCase();
    return (
      modal()
        ?.nodes.filter((n) => n.path.toLowerCase().startsWith(inside))
        .map((n) => n.path) ?? []
    );
  };

  /**
   * Opening New folder first reads what is already inside the chosen folder, so
   * a name that is taken is refused straight away and the person can see why.
   */
  const startNewFolder = async () => {
    const current = modal();
    if (!current?.picked) return;
    const parent = current.nodes.find((n) => n.path === current.picked);
    if (parent) await expand(parent);
    app.patchModal((m) => {
      if (m.kind !== "picker" || !m.picked) return;
      m.newFolder = { parent: m.picked, name: "", error: null, saving: false };
    });
  };

  const saveNewFolder = async () => {
    const current = modal();
    const draft = current?.newFolder;
    if (!current || !draft || draft.saving) return;
    const error = folderNameError(draft.name, draft.parent, siblingsOf(draft.parent));
    if (error) {
      app.patchModal((m) => {
        if (m.kind === "picker" && m.newFolder) m.newFolder.error = error;
      });
      return;
    }
    app.patchModal((m) => {
      if (m.kind === "picker" && m.newFolder) {
        m.newFolder.saving = true;
        m.newFolder.error = null;
      }
    });
    let created: string;
    let isNew: boolean;
    try {
      const result = await app.engine.createDirectory(
        joinPath(draft.parent, draft.name.trim()),
      );
      created = result.path;
      isNew = result.created;
    } catch (failure) {
      app.patchModal((m) => {
        if (m.kind === "picker" && m.newFolder) {
          m.newFolder.saving = false;
          m.newFolder.error = messageOf(failure);
        }
      });
      return;
    }
    app.patchModal((m) => {
      if (m.kind !== "picker") return;
      const parentIndex = m.nodes.findIndex((n) => n.path === draft.parent);
      const parent = m.nodes[parentIndex];
      if (parent) {
        let at = parentIndex + 1;
        while (at < m.nodes.length && m.nodes[at]!.depth > parent.depth) at += 1;
        m.nodes = [
          ...m.nodes.slice(0, at),
          {
            path: created,
            name: leafOf(created),
            isDirectory: true,
            isSymlink: false,
            isDrive: false,
            hasChildren: false,
            refusal: null,
            depth: parent.depth + 1,
            isNew: isNew,
          },
          ...m.nodes.slice(at),
        ];
        if (!m.expanded.includes(parent.path)) {
          m.expanded = [...m.expanded, parent.path];
        }
      }
      m.newFolder = null;
    });
    app.actions.showToast(
      isNew ? `Created ${created}` : `${created} was already there`,
    );
    const node = modal()?.nodes.find((n) => n.path === created);
    if (node) await pick(node);
  };

  const confirm = async () => {
    const current = modal();
    if (!current || !current.picked || confirming()) return;
    setConfirming(true);
    const path = current.picked;
    try {
      if (current.purpose === "install") {
        const root = current.candidate?.root ?? path;
        await app.engine.registerInstall(root);
        app.setModal(null);
        await app.actions.refresh();
        app.actions.showToast(`Added ${root} \u00b7 run a scan to read it`);
      } else if (current.purpose === "vault") {
        await app.engine.selectVault(path, true);
        app.setModal(null);
        await app.actions.refresh();
        app.actions.showToast(`Vault folder set to ${path}`);
      } else if (current.sha256) {
        const install = app
          .installs()
          .find((i) => path.toLowerCase().startsWith(i.root.toLowerCase() + "\\"));
        if (!install) throw new Error("That folder is not inside a registered install.");
        await app.engine.createLink({
          installId: install.id,
          sha256: current.sha256,
          relativeDir: path.slice(install.root.length + 1).replace(/\\/g, "/"),
          createDir: false,
        });
        app.setModal(null);
        await app.actions.refresh();
        app.actions.showToast(`Link created at ${path}`);
      }
    } catch (failure) {
      app.actions.showToast(messageOf(failure), "bad");
    } finally {
      setConfirming(false);
    }
  };

  const verdict = createMemo(() => {
    const current = modal();
    if (!current?.picked) return null;
    if (current.checking) return null;
    return verdictFor(app, current.purpose, current.picked, current.candidate);
  });

  const canConfirm = () => {
    const current = modal();
    if (!current) return false;
    return (
      current.picked != null &&
      verdict()?.ok === true &&
      current.newFolder === null &&
      !current.checking &&
      !confirming()
    );
  };

  return (
    <Show when={modal()}>
      {(current) => (
        <div
          class="veil"
          onClick={(e) => {
            if (e.target === e.currentTarget) app.setModal(null);
          }}
        >
          <div class="modal" role="dialog" aria-modal="true" aria-label={title()}>
            <div class="mh">
              <Icon name="folder" size={14} />
              <h2>{title()}</h2>
              <span class="sp" />
              <button
                class="tb-btn"
                aria-label="Close"
                onClick={() => app.setModal(null)}
              >
                <Icon name="x" size={12} />
              </button>
            </div>
            <div class="mb">
              <div class="note" style={{ "margin-bottom": "9px" }}>
                <Show
                  when={current().purpose === "link"}
                  fallback={
                    <>Pick a folder. There is nowhere in ComfyVault to type a path.</>
                  }
                >
                  Pick the folder inside an install where the link should appear.
                  ComfyUI will find the model at that path.
                </Show>
              </div>

              <div class="tree" role="tree">
                <For each={current().nodes}>
                  {(node) => (
                    <>
                      <div
                        class="trow"
                        style={{ "padding-left": `${node.depth * 16}px` }}
                      >
                        <Twist
                          node={node}
                          open={current().expanded.includes(node.path)}
                          busy={current().loading.includes(node.path)}
                          onToggle={() => void toggle(node)}
                        />
                        <button
                          class="tnode"
                          classList={{ on: current().picked === node.path }}
                          title={node.refusal ?? node.path}
                          onClick={() => void pick(node)}
                        >
                          <Icon name={node.isDrive ? "drive" : "folder"} size={12} />
                          <span>{node.name}</span>
                          <Show when={node.isNew}>
                            <span class="tnew-tag">new</span>
                          </Show>
                          <Show when={node.isSymlink}>
                            <span class="hint">a link to somewhere else</span>
                          </Show>
                          <Show when={node.refusal}>
                            <span class="hint red">cannot be opened</span>
                          </Show>
                        </button>
                      </div>
                      <Show when={node.refusal}>
                        {(why) => (
                          <div
                            class="tnew-err"
                            role="alert"
                            style={{ "padding-left": `${7 + node.depth * 16}px` }}
                          >
                            <Icon name="warn" size={11} />
                            <span>{why()}</span>
                          </div>
                        )}
                      </Show>
                      <Show
                        when={
                          current().newFolder?.parent === node.path
                            ? current().newFolder
                            : null
                        }
                      >
                        {(draft) => (
                          <>
                            <div
                              class="tnew"
                              style={{
                                "padding-left": `${7 + (node.depth + 1) * 16}px`,
                              }}
                            >
                              <Icon name="folder" size={12} />
                              <input
                                ref={(el) => queueMicrotask(() => el.focus())}
                                value={draft().name}
                                spellcheck={false}
                                placeholder="folder name"
                                aria-label="Name for the new folder"
                                onInput={(e) =>
                                  app.patchModal((m) => {
                                    if (m.kind === "picker" && m.newFolder) {
                                      m.newFolder.name = e.currentTarget.value;
                                      m.newFolder.error = null;
                                    }
                                  })
                                }
                                onKeyDown={(e) => {
                                  if (e.key === "Enter") {
                                    e.preventDefault();
                                    void saveNewFolder();
                                  }
                                  if (e.key === "Escape") {
                                    e.preventDefault();
                                    e.stopPropagation();
                                    app.patchModal((m) => {
                                      if (m.kind === "picker") m.newFolder = null;
                                    });
                                  }
                                }}
                              />
                              <button
                                class="btn sm pri"
                                disabled={draft().saving}
                                onClick={() => void saveNewFolder()}
                              >
                                Create
                              </button>
                              <button
                                class="btn sm"
                                onClick={() =>
                                  app.patchModal((m) => {
                                    if (m.kind === "picker") m.newFolder = null;
                                  })
                                }
                              >
                                Cancel
                              </button>
                            </div>
                            <Show when={draft().error}>
                              {(message) => (
                                <div class="tnew-err" role="alert">
                                  <Icon name="warn" size={11} />
                                  <span>{message()}</span>
                                </div>
                              )}
                            </Show>
                          </>
                        )}
                      </Show>
                    </>
                  )}
                </For>
              </div>

              <Show when={current().picked}>
                {(path) => <div class="picked">{path()}</div>}
              </Show>
              <Show when={current().checking}>
                <div class="verdict wait">
                  <h4>
                    <Icon name="clock" size={12} />
                    Looking inside
                  </h4>
                  <p>ComfyVault is reading that folder.</p>
                </div>
              </Show>
              <Show when={verdict()}>
                {(v) => (
                  <div class="verdict" classList={{ ok: v().ok, no: !v().ok }}>
                    <h4>
                      <Icon name={v().ok ? "check" : "x"} size={12} />
                      {v().title}
                    </h4>
                    {v().body}
                  </div>
                )}
              </Show>
            </div>
            <div class="mf">
              <button
                class="btn"
                disabled={!current().picked || current().newFolder !== null}
                title={
                  current().picked
                    ? `Create a folder inside ${current().picked}`
                    : "Pick a folder first"
                }
                onClick={() => void startNewFolder()}
              >
                <Icon name="plus" size={11} />
                New folder
              </button>
              <Show when={current().picked && !current().newFolder}>
                <span class="note">
                  inside {leafOf(current().picked!) || current().picked}
                </span>
              </Show>
              <span class="sp" />
              <button class="btn" onClick={() => app.setModal(null)}>
                Cancel
              </button>
              <button
                class="btn pri"
                disabled={!canConfirm()}
                onClick={() => void confirm()}
              >
                {cta()}
              </button>
            </div>
          </div>
        </div>
      )}
    </Show>
  );
}

function Twist(props: {
  node: TreeNode;
  open: boolean;
  busy: boolean;
  onToggle: () => void;
}) {
  return (
    <Show
      when={props.node.hasChildren !== false}
      fallback={<span class="twist leaf" />}
    >
      <button
        class="twist"
        classList={{ open: props.open, shut: !props.open, busy: props.busy }}
        aria-expanded={props.open}
        aria-label={
          props.open ? `Collapse ${props.node.name}` : `Open ${props.node.name}`
        }
        onClick={props.onToggle}
      >
        <Icon name={props.busy ? "refresh" : "chev"} size={11} />
      </button>
    </Show>
  );
}

// ── the verdict under the tree ──────────────────────────────────────────────

export interface VerdictCopy {
  ok: boolean;
  title: string;
  body: JSX.Element;
}

/** Every sentence the picker says about a folder it looked into. */
export function verdictFor(
  app: AppStore,
  purpose: PickerPurpose,
  path: string,
  candidate: InstallCandidate | null,
): VerdictCopy {
  if (purpose === "link") {
    const install = app
      .installs()
      .find((i) => path.toLowerCase().startsWith(i.root.toLowerCase() + "\\"));
    if (!install) {
      return {
        ok: false,
        title: "Outside every install",
        body: (
          <p>
            ComfyUI only looks inside its own folders. Pick a folder inside one of
            the installs you registered, so ComfyUI finds the model there.
          </p>
        ),
      };
    }
    const inModels =
      path.toLowerCase().startsWith(install.modelsDir.toLowerCase()) ||
      install.extraPaths.some((extra) =>
        path.toLowerCase().startsWith(extra.path.toLowerCase()),
      );
    if (!inModels) {
      return {
        ok: false,
        title: "ComfyUI does not look here",
        body: (
          <p>
            {install.label} only reads models from{" "}
            <span class="emph">{install.modelsDir}</span> and the folders its
            extra_model_paths.yaml adds. A link anywhere else would never be found.
          </p>
        ),
      };
    }
    return {
      ok: true,
      title: "Folder accepted",
      body: (
        <p>
          The link will appear at <span class="emph">{path}</span>, inside{" "}
          {install.label}. The file itself stays in the vault.
        </p>
      ),
    };
  }

  if (purpose === "vault") {
    const inside = app
      .installs()
      .find((i) => path.toLowerCase().startsWith(i.root.toLowerCase() + "\\"));
    if (inside) {
      return {
        ok: false,
        title: "That is inside an install",
        body: (
          <p>
            The vault cannot live inside <span class="emph">{inside.label}</span>.
            Removing that install later would take the vault with it. Pick a folder
            of its own.
          </p>
        ),
      };
    }
    const volume = path.slice(0, 2).toUpperCase();
    const elsewhere = app
      .installs()
      .filter((i) => i.root.slice(0, 2).toUpperCase() !== volume);
    if (elsewhere.length === 0) {
      return {
        ok: true,
        title:
          app.installs().length === 1
            ? "Same drive as the install"
            : "Same drive as every install",
        body: (
          <p>
            Files move instead of being copied. The space comes back as each one
            moves, and nothing extra is needed up front.
          </p>
        ),
      };
    }
    const needed = app.scan()?.totals.uniqueBytes ?? 0;
    return {
      ok: true,
      title: "This is on another drive",
      body: (
        <p>
          {elsewhere.map((i) => i.label).join(" and ")}{" "}
          {elsewhere.length === 1 ? "sits" : "sit"} on a different drive from{" "}
          {volume}. Every file from there is <span class="emph">copied</span> and
          checked before the original goes, so {volume} needs up to{" "}
          <span class="emph">{fmt(needed)}</span> free before the other drive gives
          anything back. ComfyVault checks the free space before it starts.
        </p>
      ),
    };
  }

  if (!candidate) {
    return {
      ok: false,
      title: "Not checked yet",
      body: <p>ComfyVault has not looked inside that folder.</p>,
    };
  }

  if (!candidate.valid) {
    const already = app
      .installs()
      .find((i) => i.root.toLowerCase() === path.toLowerCase());
    if (already) {
      return {
        ok: false,
        title: "Already registered",
        body: <p>{already.label} is this folder. Nothing to add.</p>,
      };
    }
    return {
      ok: false,
      title: "Not a ComfyUI install",
      body: (
        <>
          <p>
            {candidate.reason ??
              "ComfyVault did not find ComfyUI's own files inside that folder."}
          </p>
          <Show when={candidate.markersMissing.length > 0}>
            <p>
              It looked for {candidate.markersMissing.slice(0, 4).join(", ")} and
              did not find {candidate.markersMissing.length === 1 ? "it" : "them"}.
              Pick the folder that holds ComfyUI itself, the one with main.py in it.
            </p>
          </Show>
        </>
      ),
    };
  }

  const already = app
    .installs()
    .find((i) => i.root.toLowerCase() === (candidate.root ?? path).toLowerCase());
  if (already) {
    return {
      ok: false,
      title: "Already registered",
      body: <p>{already.label} is this install. Nothing to add.</p>,
    };
  }

  const volume = (candidate.root ?? path).slice(0, 2).toUpperCase();
  const vaultVolume = app.vault()?.volume ?? "C:";

  return {
    ok: true,
    title: "This is a ComfyUI install",
    body: (
      <>
        <Show when={candidate.nestedDepth > 0}>
          <p>
            ComfyUI itself is at <span class="emph">{candidate.root}</span>, inside
            the folder you picked. That is the one ComfyVault will read.
          </p>
        </Show>
        <p>
          Found <span class="emph">{candidate.modelsDir}</span>.{" "}
          <span class="emph">extra_model_paths.yaml</span>:{" "}
          {candidate.extraPathsFile
            ? `${candidate.extraPaths.length} extra ${candidate.extraPaths.length === 1 ? "folder" : "folders"}`
            : "not found"}
          . Every file in there is read on the next scan.
        </p>
        <Show when={candidate.extraPathsError}>
          {(problem) => (
            <p>
              Its extra_model_paths.yaml could not be read: {problem()}. ComfyVault
              will register the install and skip that file.
            </p>
          )}
        </Show>
        <Show when={candidate.otherCandidates.length > 0}>
          <p>
            There {candidate.otherCandidates.length === 1 ? "is" : "are"}{" "}
            {candidate.otherCandidates.length} more ComfyUI{" "}
            {candidate.otherCandidates.length === 1 ? "install" : "installs"} under
            that folder. Add {candidate.otherCandidates.length === 1 ? "it" : "them"}{" "}
            separately if you want them read too.
          </p>
        </Show>
        <Show when={volume !== vaultVolume}>
          <p>
            This install is on drive {volume} and the vault is on {vaultVolume}. Its
            files are <span class="emph">copied</span> and checked before the
            original goes, so {vaultVolume} pays for them first. ComfyVault checks
            the free space before it starts.
          </p>
        </Show>
      </>
    ),
  };
}
