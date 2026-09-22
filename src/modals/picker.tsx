import { For, Show, createMemo, createSignal, type JSX } from "solid-js";

import { Icon } from "~/components/Icon";
import { dayMonth, fmt, leafOf } from "~/domain/format";
import { folderNameError } from "~/domain/foldername";
import type { FolderCheck, PickerPurpose } from "~/ipc/contract";
import { useApp, type AppState, type TreeNode } from "~/state/store";

// ── opening one ─────────────────────────────────────────────────────────────

async function openPicker(
  app: AppState,
  purpose: PickerPurpose,
  extra: { modelId?: string; replacing?: string } = {},
): Promise<void> {
  const roots = await app.engine.listFolder(null, purpose);
  app.setModal({
    kind: "picker",
    purpose,
    nodes: roots.map((entry) => toNode(entry, 0)),
    expanded: [],
    loading: [],
    picked: null,
    check: null,
    checking: false,
    newFolder: null,
    modelId: extra.modelId ?? null,
    replacing: extra.replacing ?? null,
  });
}

export const openInstancePicker = (app: AppState) => openPicker(app, "instance");
export const openVaultPicker = (app: AppState) => openPicker(app, "vault");
export const openLinkPicker = (app: AppState, modelId: string) =>
  openPicker(app, "link", { modelId });
/** Point an install that is already registered at a different folder. */
export const openInstanceEditor = (app: AppState, instanceId: string) =>
  openPicker(app, "instance", { replacing: instanceId });

function instanceNameOf(app: AppState, id: string): string {
  return app.scan()?.instances.find((i) => i.id === id)?.name ?? id;
}

function toNode(
  entry: {
    path: string;
    name: string;
    kind: "drive" | "folder";
    looksLikeInstall: boolean | null;
    readable: boolean;
  },
  depth: number,
): TreeNode {
  return {
    path: entry.path,
    name: entry.name,
    kind: entry.kind,
    depth,
    looksLikeInstall: entry.looksLikeInstall,
    readable: entry.readable,
    isNew: false,
    hasChildren: null,
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
    const current = modal();
    if (current?.replacing) {
      return `Choose the folder for ${instanceNameOf(app, current.replacing)}`;
    }
    switch (current?.purpose) {
      case "vault":
        return "Choose the vault folder";
      case "link":
        return "Choose where the link goes";
      default:
        return "Choose a ComfyUI install folder";
    }
  };
  const cta = () => {
    const current = modal();
    if (current?.replacing) return "Point it here";
    switch (current?.purpose) {
      case "vault":
        return "Use this folder";
      case "link":
        return "Put the link here";
      default:
        return "Add this install";
    }
  };

  /** Everything under a folder, for the prefix test. "C:\\" has no extra slash. */
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
    const children = await app.engine.listFolder(node.path, current.purpose);
    app.patchModal((m) => {
      if (m.kind !== "picker") return;
      m.loading = m.loading.filter((p) => p !== node.path);
      m.expanded = [...m.expanded, node.path];
      const at = m.nodes.findIndex((n) => n.path === node.path);
      if (at < 0) return;
      m.nodes[at]!.hasChildren = children.length > 0;
      m.nodes = [
        ...m.nodes.slice(0, at + 1),
        ...children.map((child) => toNode(child, node.depth + 1)),
        ...m.nodes.slice(at + 1),
      ];
    });
  };

  const toggle = async (node: TreeNode) => {
    const current = modal();
    if (!current || !node.readable) return;
    if (current.expanded.includes(node.path)) collapse(node);
    else await expand(node);
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

  const pick = async (node: TreeNode) => {
    const current = modal();
    if (!current) return;
    app.patchModal((m) => {
      if (m.kind !== "picker") return;
      m.picked = node.path;
      m.check = null;
      m.checking = true;
      m.newFolder = null;
    });
    const check = await app.engine.checkFolder(node.path, current.purpose);
    app.patchModal((m) => {
      if (m.kind !== "picker" || m.picked !== node.path) return;
      m.check = check;
      m.checking = false;
    });
  };

  const siblingsOf = (parent: string) => {
    const inside = insideOf(parent).toLowerCase();
    return (
      modal()
        ?.nodes.filter((n) => n.path.toLowerCase().startsWith(inside))
        .map((n) => n.path) ?? []
    );
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
    const result = await app.engine.createFolder(draft.parent, draft.name.trim());
    if (!result.ok) {
      const message =
        result.reason === "exists"
          ? `There is already a folder called ${draft.name.trim()} here.`
          : result.reason === "denied"
            ? "Windows refused to create a folder there. Pick another place."
            : "Windows will not accept that name. Try a different one.";
      app.patchModal((m) => {
        if (m.kind === "picker" && m.newFolder) {
          m.newFolder.saving = false;
          m.newFolder.error = message;
        }
      });
      return;
    }
    const created = result.path;
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
            kind: "folder",
            depth: parent.depth + 1,
            looksLikeInstall: false,
            readable: true,
            isNew: true,
            hasChildren: false,
          },
          ...m.nodes.slice(at),
        ];
        if (!m.expanded.includes(parent.path)) {
          m.expanded = [...m.expanded, parent.path];
        }
      }
      m.newFolder = null;
    });
    app.actions.showToast(`Created ${created}`);
    const node = modal()?.nodes.find((n) => n.path === created);
    if (node) await pick(node);
  };

  const confirm = async () => {
    const current = modal();
    if (!current || !current.picked || confirming()) return;
    setConfirming(true);
    try {
      const path = current.picked;
      if (current.purpose === "instance") {
        await app.engine.addInstance(path);
        await app.actions.refresh();
        app.setModal(null);
        if (app.scan()?.models.length) {
          app.actions.showToast(`Added ${path} · run a scan to read it`);
        } else {
          void app.engine.startScan();
        }
      } else if (current.purpose === "vault") {
        await app.engine.setVaultPath(path);
        await app.actions.refresh();
        app.setModal(null);
        app.actions.showToast(`Vault folder set to ${path}`);
      } else {
        if (current.modelId) await app.engine.addLink(current.modelId, path);
        await app.actions.refresh();
        app.setModal(null);
        app.actions.showToast(`Link created at ${path}`);
      }
    } finally {
      setConfirming(false);
    }
  };

  const canConfirm = createMemo(() => {
    const current = modal();
    if (!current) return false;
    return (
      current.picked != null &&
      current.check?.ok === true &&
      current.newFolder === null &&
      !current.checking &&
      !confirming()
    );
  });

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
                  fallback={<>Pick a folder. There is nowhere in ComfyVault to type a path.</>}
                >
                  Pick the folder inside this install where the link should appear.
                  ComfyUI will find the model at that path.
                </Show>
              </div>

              <div class="tree" role="tree">
                <For each={current().nodes}>
                  {(node) => (
                    <>
                      <div class="trow" style={{ "padding-left": `${node.depth * 16}px` }}>
                        <Twist
                          node={node}
                          open={current().expanded.includes(node.path)}
                          busy={current().loading.includes(node.path)}
                          onToggle={() => void toggle(node)}
                        />
                        <button
                          class="tnode"
                          classList={{ on: current().picked === node.path }}
                          disabled={!node.readable}
                          title={node.readable ? node.path : `${node.path} cannot be opened`}
                          onClick={() => void pick(node)}
                        >
                          <Icon name={node.kind === "drive" ? "drive" : "folder"} size={12} />
                          <span>{node.name}</span>
                          <Show when={node.isNew}>
                            <span class="tnew-tag">new</span>
                          </Show>
                          <Show when={node.looksLikeInstall === true}>
                            <span class="hint">has a models folder</span>
                          </Show>
                          <Show when={!node.readable}>
                            <span class="hint">cannot be opened</span>
                          </Show>
                        </button>
                      </div>
                      <Show when={current().newFolder?.parent === node.path ? current().newFolder : null}>
                        {(draft) => (
                          <>
                            <div
                              class="tnew"
                              style={{ "padding-left": `${7 + (node.depth + 1) * 16}px` }}
                            >
                              <Icon name="folder" size={12} />
                              <input
                                id="newfoldername"
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
                              {(error) => (
                                <div class="tnew-err" role="alert">
                                  <Icon name="warn" size={11} />
                                  <span>{error()}</span>
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
              <Verdict />
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
      when={props.node.readable && props.node.hasChildren !== false}
      fallback={<span class="twist leaf" />}
    >
      <button
        class="twist"
        classList={{ open: props.open, shut: !props.open, busy: props.busy }}
        aria-expanded={props.open}
        aria-label={props.open ? `Collapse ${props.node.name}` : `Open ${props.node.name}`}
        onClick={props.onToggle}
      >
        <Icon name={props.busy ? "refresh" : "chev"} size={11} />
      </button>
    </Show>
  );
}

// ── the verdict under the tree ──────────────────────────────────────────────

function Verdict() {
  const app = useApp();
  const modal = () => {
    const current = app.modal();
    return current && current.kind === "picker" ? current : null;
  };

  return (
    <Show when={modal()?.picked}>
      <Show
        when={!modal()?.checking}
        fallback={
          <div class="verdict wait">
            <h4>
              <Icon name="clock" size={12} />
              Looking inside
            </h4>
            <p>ComfyVault is reading that folder.</p>
          </div>
        }
      >
        <Show when={modal()?.check}>
          {(check) => {
            const verdict = verdictFor(
              check(),
              modal()!.picked!,
              app,
              modal()!.replacing,
            );
            return (
              <div class="verdict" classList={{ ok: verdict.ok, no: !verdict.ok }}>
                <h4>
                  <Icon name={verdict.ok ? "check" : "x"} size={12} />
                  {verdict.title}
                </h4>
                {verdict.body}
              </div>
            );
          }}
        </Show>
      </Show>
    </Show>
  );
}

interface VerdictCopy {
  ok: boolean;
  title: string;
  body: JSX.Element;
}

/** Every sentence the picker says about a folder it looked into. */
export function verdictFor(
  check: FolderCheck,
  path: string,
  app: AppState,
  replacing: string | null = null,
): VerdictCopy {
  const instanceName = (id: string) => instanceNameOf(app, id);

  if (check.for === "link") {
    if (check.ok) {
      return {
        ok: true,
        title: "Folder accepted",
        body: (
          <p>
            The link will appear at <span class="emph">{path}</span>. The file
            itself stays in the vault.
          </p>
        ),
      };
    }
    if (check.reason === "outside_every_install") {
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
    if (check.reason === "already_holds_this_name") {
      return {
        ok: false,
        title: "A file of that name is already here",
        body: (
          <p>
            This folder already holds{" "}
            <span class="emph">{check.filename}</span>. Putting a link there would
            replace it. Pick another folder.
          </p>
        ),
      };
    }
    return {
      ok: false,
      title: "Windows will not write here",
      body: <p>ComfyVault has no permission to create anything in that folder.</p>,
    };
  }

  if (check.for === "vault") {
    if (!check.ok) {
      if (check.reason === "inside_an_install") {
        return {
          ok: false,
          title: "That is inside an install",
          body: (
            <p>
              The vault cannot live inside{" "}
              <span class="emph">{instanceName(check.instanceId)}</span>. Removing
              that install later would take the vault with it. Pick a folder of its
              own.
            </p>
          ),
        };
      }
      return {
        ok: false,
        title: "Windows will not write here",
        body: <p>ComfyVault has no permission to create anything in that folder.</p>,
      };
    }
    if (check.sameDriveAsInstalls) {
      return {
        ok: true,
        title: `Same drive as ${app.scan()!.instances.length === 1 ? "the install" : "every install"}`,
        body: (
          <p>
            Files move instead of being copied. The space comes back as each one
            moves, and nothing extra is needed up front.
          </p>
        ),
      };
    }
    const needed = app.plan()?.totals.uniqueBytes ?? 0;
    return {
      ok: true,
      title: "This is on another drive",
      body: (
        <p>
          {check.installsOnOtherDrives.join(" and ")}{" "}
          {check.installsOnOtherDrives.length === 1 ? "sits" : "sit"} on a different
          drive from {check.drive}. Every file from there is{" "}
          <span class="emph">copied</span>, not moved, so {check.drive} needs{" "}
          <span class="emph">{fmt(needed)}</span> free before the other drive gives
          anything back. Confirm this and ComfyVault will check the free space
          first.
        </p>
      ),
    };
  }

  if (!check.ok) {
    if (check.reason === "already_registered") {
      if (replacing && check.instanceId === replacing) {
        return {
          ok: false,
          title: "This is the folder it already uses",
          body: (
            <p>
              {instanceName(replacing)} points here now. Pick a different folder,
              or cancel and nothing changes.
            </p>
          ),
        };
      }
      return {
        ok: false,
        title: "Already registered",
        body: (
          <p>
            {replacing
              ? `This folder is ${instanceName(check.instanceId)}. Two installs cannot share one folder.`
              : "This install is in the list already. Nothing to add."}
          </p>
        ),
      };
    }
    if (check.reason === "unreadable") {
      return {
        ok: false,
        title: "Windows will not open that folder",
        body: (
          <p>
            ComfyVault cannot read <span class="emph">{path}</span>, so it cannot
            tell what is inside. Pick a folder you own.
          </p>
        ),
      };
    }
    return {
      ok: false,
      title: "Not a ComfyUI install",
      body: (
        <p>
          ComfyVault looked for a <span class="emph">models</span> folder inside{" "}
          <span class="emph">{path}</span> and did not find one. Pick the folder
          that holds ComfyUI itself, the one with main.py in it.
        </p>
      ),
    };
  }

  const removed = app.scan()?.removedInstance;
  const orphans = app.plan()?.orphans.length ?? 0;
  return {
    ok: true,
    title: "This is a ComfyUI install",
    body: (
      <>
        <p>
          Found <span class="emph">models\</span> with {check.modelFolders} folders,{" "}
          {check.files} files, <span class="emph">{fmt(check.bytes)}</span>.{" "}
          <span class="emph">extra_model_paths.yaml</span>:{" "}
          {check.hasExtraModelPaths ? "found" : "not found"}. Every file in there is
          read on the next scan.
        </p>
        <Show when={check.onDifferentDrive}>
          <p>
            This install is on drive {path.slice(0, 2)} and the vault is on{" "}
            {app.machine()!.vaultPath.slice(0, 2)}. Its files are{" "}
            <span class="emph">copied</span>, not moved, so{" "}
            {app.machine()!.vaultPath.slice(0, 2)} needs{" "}
            <span class="emph">{fmt(check.bytes)}</span> free before drive{" "}
            {path.slice(0, 2)} gives anything back. ComfyVault checks the free space
            before it starts.
          </p>
        </Show>
        <Show when={check.wasRemoved && removed}>
          <p>
            You removed this install on {dayMonth(removed!.removedAt)}. Adding it
            back relinks the {orphans} vault files that lost their link, so they
            stop showing up in Cleanup.
          </p>
        </Show>
      </>
    ),
  };
}
