import { For, Show, createMemo } from "solid-js";

import { Icon } from "~/components/Icon";
import { Header } from "~/components/Shell";
import { dayMonth, driveOf, fmt } from "~/domain/format";
import { openConfirm } from "~/modals/confirm";
import {
  openInstanceEditor,
  openInstancePicker,
  openVaultPicker,
} from "~/modals/picker";
import { useApp } from "~/state/store";
import type { Instance } from "~/ipc/contract";

export function SettingsScreen() {
  const app = useApp();
  const machine = () => app.machine()!;
  const plan = () => app.plan();
  const scan = () => app.scan();

  const developerMode = () => machine().developerMode;
  const running = () => machine().running;

  const recheckDeveloperMode = async () => {
    const next = await app.engine.readMachine();
    await app.actions.refresh();
    app.actions.showToast(
      next.developerMode
        ? "Checked · Developer Mode is on, links can be made"
        : "Checked · Developer Mode is still off",
      next.developerMode ? "ok" : "bad",
    );
  };

  const recheckComfy = async () => {
    const next = await app.engine.readMachine();
    await app.actions.refresh();
    if (next.running.length === 0) {
      app.actions.showToast(
        `Checked · no ComfyUI is running, ${fmt(app.plan()?.totals.reclaimBytes ?? 0)} can now come back`,
      );
    } else {
      app.actions.showToast(
        `Checked · ${next.running.map((p) => instanceName(p.instanceId)).join(" and ")} is still running`,
        "bad",
      );
    }
  };

  const instanceName = (id: string) =>
    scan()?.instances.find((i) => i.id === id)?.name ?? id;

  const removeInstance = (instance: Instance) => {
    openConfirm(app, {
      title: "Remove an install",
      cta: "Remove it",
      body: [
        [
          { text: "ComfyVault stops reading " },
          { text: instance.path, emph: true },
          {
            text: ". The links inside it are left exactly as they are, so that install keeps working. Vault files nothing else points at will show up in Cleanup.",
          },
        ],
      ],
      action: async () => {
        await app.engine.removeInstance(instance.id);
        await app.actions.refresh();
        app.actions.showToast(`Removed ${instance.name}`);
      },
    });
  };

  const vaultHeld = createMemo(() => {
    const p = plan();
    if (!p) return 0;
    return app.lastRun() ? p.totals.uniqueBytes : p.totals.vaultOnlyBytes;
  });

  const otherDrives = createMemo(() => {
    const letter = driveOf(machine().vaultPath);
    return (scan()?.instances ?? []).filter((i) => driveOf(i.path) !== letter);
  });

  return (
    <>
      <Header title="Settings" />
      <div class="screen">
        <div class="scroll">
          <div class="sec">
            <span class="t">ComfyUI installs</span>
            <span class="n">{scan()?.instances.length ?? 0} registered</span>
          </div>
          <Show
            when={(scan()?.instances.length ?? 0) > 0}
            fallback={
              <div class="note" style={{ padding: "6px 0 10px" }}>
                No install is registered. ComfyVault has nothing to read until one
                is.
              </div>
            }
          >
            <For each={scan()!.instances}>
              {(instance) => {
                const totals = () => plan()?.totals.perInstance.get(instance.id);
                return (
                  <div class="icard">
                    <span
                      class="led"
                      classList={{ up: instance.running, idle: !instance.running }}
                    />
                    <div class="it">
                      <div class="nm">{instance.name}</div>
                      <div class="pp">
                        {instance.path}
                        <Show when={instance.extraModelPaths?.length}>
                          {" "}
                          &nbsp;
                          <span class="amb">
                            + extra_model_paths.yaml &rarr;{" "}
                            {instance.extraModelPaths!.join(", ")}
                          </span>
                        </Show>
                      </div>
                    </div>
                    <div class="st">
                      <div class="a">{fmt(totals()?.bytes ?? 0)}</div>
                      <div class="b">{totals()?.files ?? 0} files</div>
                    </div>
                    <button
                      class="btn sm"
                      title={`Point ${instance.name} at a different folder`}
                      onClick={() => void openInstanceEditor(app, instance.id)}
                    >
                      Edit
                    </button>
                    <button
                      class="btn sm dng"
                      onClick={() => removeInstance(instance)}
                    >
                      Remove
                    </button>
                  </div>
                );
              }}
            </For>
          </Show>
          <div class="settings-acts">
            <button class="btn pri" onClick={() => void openInstancePicker(app)}>
              <Icon name="plus" size={13} />
              Add an install
            </button>
            <span class="note">
              ComfyVault checks the folder before it accepts it.
            </span>
          </div>
          <Show when={scan()?.removedInstance && (plan()?.orphans.length ?? 0) > 0}>
            <div class="note up">
              {scan()!.removedInstance!.name} was removed on{" "}
              {dayMonth(scan()!.removedInstance!.removedAt)}.{" "}
              {plan()!.orphans.length} vault files it used are still here, with
              nothing pointing at them.{" "}
              <button class="lnk" onClick={() => app.actions.go("cleanup")}>
                See them in Cleanup
              </button>
            </div>
          </Show>

          <div class="sec secgap">
            <span class="t">The vault folder</span>
          </div>
          <div class="card row-card">
            <Icon name="vault" size={16} />
            <div class="it" style={{ flex: 1, "min-width": 0 }}>
              <div class="nm" style={{ "font-size": "11.5px", color: "var(--t-bright)" }}>
                {machine().vaultPath}
              </div>
              <div
                class="pp"
                style={{ "font-size": "9.5px", color: "var(--t-dim)", "margin-top": "2px" }}
              >
                drive {driveOf(machine().vaultPath)} &middot; {fmt(vaultHeld())} held
                <Show when={!app.lastRun() && (plan()?.orphans.length ?? 0) > 0}>
                  {" "}
                  &middot; {plan()!.orphans.length} files
                </Show>
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
                  {otherDrives().map((i) => i.name).join(" and ")}{" "}
                  {otherDrives().length === 1 ? "sits" : "sit"} on another drive, so
                  files from there are copied rather than moved and drive{" "}
                  {driveOf(machine().vaultPath)} pays for them first. Choose a
                  folder on another drive and ComfyVault will say what changes
                  before anything happens.
                </>
              }
            >
              Every install is on drive {driveOf(machine().vaultPath)}, so files
              move instead of being copied and the space comes back straight away.
              Choose a folder on another drive and ComfyVault will say what changes
              before anything happens.
            </Show>
          </div>

          <div class="sec secgap">
            <span class="t">What Windows allows</span>
          </div>
          <div class="chk">
            <Show
              when={developerMode()}
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
            <span class="lb">Developer Mode</span>
            <span class="sp" classList={{ dim: developerMode(), red: !developerMode() }}>
              {developerMode()
                ? "on · ComfyVault can create links"
                : "off · ComfyVault cannot create links, so Apply is held back"}
            </span>
            <Show when={!developerMode()}>
              <button
                class="btn sm"
                onClick={() => void app.engine.openWindowsDeveloperSettings()}
              >
                <Icon name="external" size={11} />
                Open that page
              </button>
            </Show>
            <button
              class="btn sm"
              classList={{ pri: !developerMode() }}
              onClick={() => void recheckDeveloperMode()}
            >
              Check again
            </button>
          </div>
          <div class="chk">
            <Show
              when={running().length > 0}
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
            <span class="sp" classList={{ dim: running().length === 0, red: running().length > 0 }}>
              {running().length > 0
                ? `${running().map((p) => instanceName(p.instanceId)).join(" and ")} ${running().length === 1 ? "is" : "are"} running · ${running().length === 1 ? "its" : "their"} open files cannot move`
                : "none running · every file can move"}
            </span>
            <button
              class="btn sm"
              classList={{ pri: running().length > 0 }}
              onClick={() => void recheckComfy()}
            >
              Check again
            </button>
          </div>
          <div class="chk">
            <Icon name="check" size={13} />
            <span class="lb">Free space</span>
            <span class="sp dim">
              {fmt(machine().vaultDrive.freeBytes)} free &middot; the vault needs
              none of it, the files already exist on this drive
            </span>
          </div>

          <div class="sec secgap">
            <span class="t">Civitai lookup</span>
          </div>
          <div class="chk">
            <button
              class="tog"
              classList={{ on: scan()?.civitaiEnabled ?? false }}
              role="switch"
              aria-checked={scan()?.civitaiEnabled ?? false}
              aria-label="Ask Civitai what each file is"
              onClick={async () => {
                await app.engine.setCivitaiEnabled(!(scan()?.civitaiEnabled ?? false));
                await app.actions.refresh();
              }}
            >
              <i />
            </button>
            <span class="sp" style={{ "font-size": "10.5px" }}>
              Ask Civitai what each file is, using its fingerprint
            </span>
          </div>
          <div class="note up">
            The fingerprint goes out, nothing else. No filename, no path. Turn it
            off and ComfyVault runs with no network at all. Files Civitai does not
            know stay unlabelled, which is normal.
          </div>
        </div>
      </div>
    </>
  );
}
