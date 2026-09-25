import { For, Show, createMemo } from "solid-js";

import { Icon } from "~/components/Icon";
import { Header } from "~/components/Shell";
import { driveKindShort, isReadable } from "~/domain/drives";
import { dayMonth, driveOf, fmt } from "~/domain/format";
import { ThumbnailNote } from "~/components/ThumbnailNote";
import { openConfirm } from "~/modals/confirm";
import { openInstallPicker, openVaultPicker } from "~/modals/picker";
import { Boundary } from "~/components/Boundary";
import { useApp } from "~/state/store";
import { cacheDirsOf, type Install } from "~/ipc/contract";

/** What a settings row says when the engine sent no answer for it. */
const UNKNOWN = "not known";



/**
 * A yes or a no from the engine, or nothing at all. A field the engine did not
 * send is not a "no": saying "ignored" when nobody said so is a claim about
 * what a scan touches, and this screen does not make claims it was not told.
 */
function said(value: boolean | undefined, yes: string, no: string): string {
  if (value === true) return yes;
  if (value === false) return no;
  return UNKNOWN;
}

export function SettingsScreen() {
  const app = useApp();
  const platform = () => app.appState()?.platform ?? null;
  const settings = () => app.appState()?.settings ?? null;
  const vault = () => app.vault();

  const recheck = async () => {
    await app.actions.refresh();
    const links = platform()?.symlinks.supported ?? false;
    const running = app.running().length;
    app.actions.showToast(
      links && running === 0
        ? "Checked · nothing is in the way"
        : !links
          ? "Checked · Windows still will not create links"
          : `Checked · ${running} ComfyUI ${running === 1 ? "process is" : "processes are"} still running`,
      links && running === 0 ? "ok" : "bad",
    );
  };

  const removeInstall = (install: Install) => {
    openConfirm(app, {
      title: "Remove an install",
      cta: "Remove it",
      body: [
        [
          { text: "ComfyVault stops reading " },
          { text: install.root, emph: true },
          {
            text: ". Every link inside it is left exactly as it is, so that install keeps working and nothing it loads breaks. Vault files that nothing else points at will show up in Cleanup.",
          },
        ],
      ],
      action: async () => {
        const result = await app.engine.unregisterInstall(install.id);
        app.actions.showToast(
          `Removed ${install.label} · ${result.linksLeftInPlace} links left exactly where they are`,
        );
      },
    });
  };

  const otherDrives = createMemo(() => {
    const letter = driveOf(vault()?.root ?? "C:\\");
    return app.installs().filter((i) => driveOf(i.root) !== letter);
  });

  return (
    <>
      <Header title="Settings" />
      <div class="screen">
        <div class="scroll">
          <div class="sec">
            <span class="t">ComfyUI installs</span>
            <span class="n">{app.installs().length} registered</span>
          </div>
          <Show
            when={app.installs().length > 0}
            fallback={
              <div class="note" style={{ padding: "6px 0 10px" }}>
                No install is registered. ComfyVault has nothing to read until one
                is.
              </div>
            }
          >
            <For each={app.installViews()}>
              {(view) => (
                <div class="icard">
                  <span
                    class="led"
                    classList={{ up: view.running, idle: !view.running }}
                  />
                  <div class="it">
                    <div class="nm">
                      {view.install.label}
                      <span class="faint" style={{ "font-weight": 400 }}>
                        {" "}
                        &middot;{" "}
                        {view.install.version ?? "version unknown"}
                      </span>
                    </div>
                    <div class="pp">
                      {view.install.root}
                      <Show when={view.install.extraPaths.length > 0}>
                        {" "}
                        &nbsp;
                        <span class="amb">
                          + extra_model_paths.yaml &rarr;{" "}
                          {view.install.extraPaths.map((p) => p.path).join(", ")}
                        </span>
                      </Show>
                    </div>
                  </div>
                  <div class="st">
                    <div class="a">{fmt(view.bytes)}</div>
                    <div class="b">{view.files} files</div>
                  </div>
                  <button
                    class="btn sm"
                    onClick={() =>
                      void app.actions.run(
                        () => app.engine.refreshInstall(view.install.id),
                        `Re-read ${view.install.label}`,
                      )
                    }
                  >
                    Re-read
                  </button>
                  <button
                    class="btn sm dng"
                    onClick={() => removeInstall(view.install)}
                  >
                    Remove
                  </button>
                </div>
              )}
            </For>
          </Show>
          <div class="settings-acts">
            {/* Measured against the real engine: before a vault exists the
                folder check accepts an install and the engine then refuses to
                register it, so the refusal would be the first thing the person
                learns. The same rule as Home: no vault, no install step. */}
            <button
              class="btn pri"
              disabled={!app.hasVault()}
              title={
                app.hasVault()
                  ? undefined
                  : "The vault has to exist before an install can point into it"
              }
              onClick={() => void openInstallPicker(app)}
            >
              <Icon name="plus" size={13} />
              Add an install
            </button>
            <span class="note">
              {app.hasVault()
                ? "ComfyVault checks the folder before it accepts it."
                : "Choose the vault folder first. Every install points into it."}
            </span>
          </div>
          <Show when={app.orphans().length > 0}>
            <div class="note up">
              {app.orphans().length} vault{" "}
              {app.orphans().length === 1 ? "file has" : "files have"} nothing
              pointing at them.{" "}
              <button class="lnk" onClick={() => app.actions.go("cleanup")}>
                See them in Cleanup
              </button>
            </div>
          </Show>
          <ThumbnailNote />

          <div class="sec secgap">
            <span class="t">The vault folder</span>
          </div>
          <Show when={vault()}>
            {(info) => (
              <>
                <div class="card row-card">
                  <Icon name="vault" size={16} />
                  <div class="it" style={{ flex: 1, "min-width": 0 }}>
                    <div
                      class="nm"
                      style={{ "font-size": "11.5px", color: "var(--t-bright)" }}
                    >
                      {info().root}
                    </div>
                    <div
                      class="pp"
                      style={{
                        "font-size": "9.5px",
                        color: "var(--t-dim)",
                        "margin-top": "2px",
                      }}
                    >
                      drive {app.vaultVolume()} &middot; {fmt(info().totalStoredBytes)}{" "}
                      held &middot; {info().fileCount} files
                    </div>
                  </div>
                  <button class="btn sm" onClick={() => void openVaultPicker(app)}>
                    <Icon name="folder" size={11} />
                    Change
                  </button>
                </div>
                <div class="note up">
                  <Show
                    when={otherDrives().length === 0}
                    fallback={
                      <>
                        {otherDrives().map((i) => i.label).join(" and ")}{" "}
                        {otherDrives().length === 1 ? "sits" : "sit"} on another
                        drive, so files from there are copied across and checked
                        before the original goes, and drive {app.vaultVolume()} pays for
                        them first.
                      </>
                    }
                  >
                    Every install is on drive {app.vaultVolume()}, so files move instead
                    of being copied and the space comes back straight away. Choose a
                    folder on another drive and ComfyVault will say what changes
                    before anything happens.
                  </Show>
                </div>
              </>
            )}
          </Show>
          <Show when={!vault()}>
            <div class="card row-card">
              <Icon name="vault" size={16} />
              <div class="it" style={{ flex: 1, "min-width": 0 }}>
                <div
                  class="nm"
                  style={{ "font-size": "11.5px", color: "var(--t-muted)" }}
                >
                  Not chosen yet
                </div>
                <div
                  class="pp"
                  style={{
                    "font-size": "9.5px",
                    color: "var(--t-dim)",
                    "margin-top": "2px",
                  }}
                >
                  {/* Readable drives only: a drive that did not answer has no
                      figure to print, and printing one anyway is how "has
                      undefined MB free" happens. */}
                  {app
                    .drives()
                    .filter(isReadable)
                    .map((d) => {
                      const kind = driveKindShort(d.kind);
                      return `${d.root.replace(/\\+$/, "")} has ${fmt(d.freeBytes!)} free${kind ? ` (${kind})` : ""}`;
                    })
                    .join(" · ")}
                </div>
              </div>
              <button class="btn sm pri" onClick={() => void openVaultPicker(app)}>
                <Icon name="folder" size={11} />
                Choose it
              </button>
            </div>
            <div class="note up">
              Put it on the same drive as your installs. Files are moved there
              rather than copied, so the vault needs no free space of its own. A
              folder on another drive means every file is copied across first, and
              ComfyVault will say what that needs before anything happens.
            </div>
          </Show>

          <div class="sec secgap">
            <span class="t">What Windows allows</span>
          </div>
          <Show when={platform()}>
            {(report) => (
              <>
                <div class="chk">
                  <Show
                    when={report().symlinks.supported}
                    fallback={
                      <span class="red">
                        <Icon name="x" size={13} />
                      </span>
                    }
                  >
                    <span class="grn">
                      <Icon name="check" size={13} />
                    </span>
                  </Show>
                  <span class="lb">Links</span>
                  <span
                    class="sp"
                    classList={{
                      dim: report().symlinks.supported,
                      red: !report().symlinks.supported,
                    }}
                  >
                    {report().symlinks.supported
                      ? "ComfyVault made a test link and removed it again, so links work"
                      : "ComfyVault could not make a test link, so Apply is held back"}
                  </span>
                  <Show when={!report().symlinks.supported}>
                    <button
                      class="btn sm"
                      onClick={() =>
                        void app.engine.openExternal("ms-settings:developers")
                      }
                    >
                      <Icon name="external" size={11} />
                      Open that page
                    </button>
                  </Show>
                  <button
                    class="btn sm"
                    classList={{ pri: !report().symlinks.supported }}
                    onClick={() => void recheck()}
                  >
                    Check again
                  </button>
                </div>
                <Show when={!report().symlinks.supported && report().symlinks.guidance}>
                  {(guidance) => <div class="note up">{guidance()}</div>}
                </Show>
                <div class="chk">
                  <Show
                    when={app.running().length > 0}
                    fallback={
                      <span class="grn">
                        <Icon name="check" size={13} />
                      </span>
                    }
                  >
                    <span class="red">
                      <Icon name="warn" size={13} />
                    </span>
                  </Show>
                  <span class="lb">ComfyUI processes</span>
                  <span
                    class="sp"
                    classList={{
                      dim: app.running().length === 0,
                      red: app.running().length > 0,
                    }}
                  >
                    {app.running().length > 0
                      ? `${app.running().map((p) => `${p.name} (pid ${p.pid})`).join(", ")} · files they hold open cannot move`
                      : "none running · every file can move"}
                  </span>
                  <button
                    class="btn sm"
                    classList={{ pri: app.running().length > 0 }}
                    onClick={() => void recheck()}
                  >
                    Check again
                  </button>
                </div>
                <div class="chk">
                  <Show
                    when={vault()?.freeBytes != null}
                    fallback={<Icon name="clock" size={13} />}
                  >
                    <Icon name="check" size={13} />
                  </Show>
                  <span class="lb">Free space</span>
                  <span class="sp dim">
                    <Show
                      when={vault()}
                      fallback={
                        <>
                          not known until a vault folder is chosen, because free
                          space is a fact about the drive it goes on
                        </>
                      }
                    >
                      {(info) => (
                        <Show
                          when={info().freeBytes !== null}
                          fallback={
                            <>
                              drive {app.vaultVolume()} did not answer when asked
                              how much room it has
                            </>
                          }
                        >
                          {fmt(info().freeBytes!)} free &middot; a file that is
                          already on this drive is renamed into the vault, so it
                          needs none of it
                        </Show>
                      )}
                    </Show>
                  </span>
                </div>
                <Show when={report().longPathsEnabled === false}>
                  <div class="chk">
                    <span class="red">
                      <Icon name="warn" size={13} />
                    </span>
                    <span class="lb">Long paths</span>
                    <span class="sp red">
                      Windows is limited to 260 characters per path, so a deeply
                      nested model folder can fail to move
                    </span>
                  </div>
                </Show>
              </>
            )}
          </Show>

          <div class="sec secgap">
            <span class="t">Civitai lookup</span>
          </div>
          <Boundary where="Civitai lookup">
          <Show when={settings()}>
            {(current) => (
              <>
                <div class="chk">
                  <button
                    class="tog"
                    classList={{ on: current().metadataLookupsEnabled === true }}
                    role="switch"
                    aria-checked={current().metadataLookupsEnabled === true}
                    aria-label="Ask Civitai what each file is"
                    onClick={() =>
                      void app.actions.run(() =>
                        app.engine.updateSettings({
                          metadataLookupsEnabled: !current().metadataLookupsEnabled,
                        }),
                      )
                    }
                  >
                    <i />
                  </button>
                  <span class="sp" style={{ "font-size": "10.5px" }}>
                    Ask Civitai what each file is, using its fingerprint
                  </span>
                </div>
                <div class="note up">
                  The fingerprint goes out, nothing else. No filename, no path. Turn
                  it off and ComfyVault runs with no network at all. Files Civitai
                  does not know stay unlabelled, which is normal.
                </div>
              </>
            )}
          </Show>

          <div class="sec secgap">
            <span class="t">What a scan reads</span>
          </div>
          <Boundary where="What a scan reads">
          <Show when={settings()}>
            {(current) => (
              <>
                <div class="kv">
                  <span class="k w150">File types</span>
                  <span class="v">
                    {current().scanExtensions?.length
                      ? current().scanExtensions.join("  ")
                      : UNKNOWN}
                  </span>
                </div>
                <div class="kv">
                  <span class="k w150">Smallest file</span>
                  <span class="v">
                    {Number.isFinite(current().minFileSizeBytes)
                      ? fmt(current().minFileSizeBytes)
                      : UNKNOWN}
                  </span>
                </div>
                <div class="kv">
                  <span class="k w150">Extra model paths</span>
                  <span class="v">
                    {said(
                      current().followExtraModelPaths,
                      "followed, as ComfyUI follows them",
                      "ignored",
                    )}
                  </span>
                </div>
                <div class="kv">
                  <span class="k w150">Hugging Face cache</span>
                  <span class="v">
                    {cacheDirsOf(current()) === undefined
                      ? UNKNOWN
                      : cacheDirsOf(current()) === null
                        ? "found the way the Hugging Face libraries find it themselves"
                        : cacheDirsOf(current())!.length === 0
                          ? "not read"
                          : cacheDirsOf(current())!.join("  ")}
                  </span>
                </div>
                <div class="kv">
                  <span class="k w150">Reading again</span>
                  <span class="v">
                    {said(
                      current().hashCacheEnabled,
                      "a file is only read again when its size or date changed",
                      "every file is read in full on every scan",
                    )}
                  </span>
                </div>
                <Show when={app.scan()}>
                  {(scan) => (
                    <div class="note up">
                      The last scan read {fmt(scan().totals.bytesRead)} and took{" "}
                      {Math.round(scan().totals.durationMs / 1000)} seconds.
                      <Show when={scan().totals.bytesFromCache > 0}>
                        {" "}
                        {fmt(scan().totals.bytesFromCache)} was already known and
                        not read again.
                      </Show>
                      <Show when={scan().errors.length > 0}>
                        {" "}
                        {scan().errors.length} files could not be read.
                      </Show>
                    </div>
                  )}
                </Show>
              </>
            )}
          </Show>

          </Boundary>

          <Show when={app.scan()?.errors.length}>
            <div class="sec secgap plain">
              <span class="t">Files the last scan could not read</span>
              <span class="n">{app.scan()!.errors.length}</span>
            </div>
            <For each={app.scan()!.errors.slice(0, 10)}>
              {(error) => (
                <div class="kv">
                  <span class="k w150">{error.code}</span>
                  <span class="v">
                    {error.path} <span class="dim">&middot; {error.detail}</span>
                  </span>
                </div>
              )}
            </For>
          </Show>

          </Boundary>

          <div class="sec secgap">
            <span class="t">Before a copy is deleted</span>
          </div>
          <Boundary where="Before a copy is deleted">
          <Show when={settings()}>
            {(current) => (
              <>
                <div class="chk">
                  <button
                    class="tog"
                    classList={{ on: current().verifyBeforeDelete === true }}
                    role="switch"
                    aria-checked={current().verifyBeforeDelete === true}
                    aria-label="Read both files again before deleting a duplicate"
                    onClick={() =>
                      void app.actions.run(() =>
                        app.engine.updateSettings({
                          verifyBeforeDelete: !current().verifyBeforeDelete,
                        }),
                      )
                    }
                  >
                    <i />
                  </button>
                  <span class="sp" style={{ "font-size": "10.5px" }}>
                    Read both files again, right before the duplicate is deleted
                  </span>
                </div>
                <div class="note up">
                  Deleting is the one thing ComfyVault does that cannot be undone.
                  With this on, the two files are read and compared at the moment of
                  the delete, so what proves they are identical is the bytes
                  themselves rather than a hash from an earlier scan. Turning it off
                  makes a run faster and makes the delete a matter of trust rather
                  than proof.
                </div>
              </>
            )}
          </Show>

          </Boundary>

          <Show when={app.appState()?.vaultRoot}>
            <div class="sec secgap plain">
              <span class="t">The vault's own health</span>
            </div>
            <div class="settings-acts">
              <button
                class="btn sm"
                onClick={() =>
                  void (async () => {
                    // Refresh so the whole interface learns what the check found,
                    // not just this toast: a broken link has a panel of its own.
                    await app.actions.refresh();
                    const health = app.health();
                    const broken = app.danglingLinks().length;
                    app.actions.showToast(
                      broken > 0
                        ? `${broken} ${broken === 1 ? "link points" : "links point"} at a file that is not there · see Cleanup`
                        : `Checked ${health?.checkedLinks ?? 0} links and ${health?.checkedFiles ?? 0} files · all well`,
                      broken > 0 ? "bad" : "ok",
                    );
                    if (broken > 0) app.actions.go("cleanup");
                  })()
                }
              >
                <Icon name="refresh" size={11} />
                Check every link
              </button>
              <span class="note">
                A link that points at a missing file shows up in ComfyUI and then
                fails to load, so it is worth catching.
              </span>
            </div>
          </Show>

          <Show when={app.installs().length > 0 && app.installs()[0]!.addedAt}>
            <div class="note up" style={{ "margin-top": "18px" }}>
              First install registered on {dayMonth(app.installs()[0]!.addedAt)}.
            </div>
          </Show>
        </div>
      </div>
    </>
  );
}
