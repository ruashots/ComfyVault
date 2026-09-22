import { For, Show, createMemo, type JSX } from "solid-js";

import { Icon, Mark, type IconName } from "~/components/Icon";
import { fmt, usedPercent } from "~/domain/format";
import { applyBlockers } from "~/domain/selection";
import { useApp, type Screen } from "~/state/store";

/** The window's own title bar. The window has no system frame. */
export function Titlebar() {
  const app = useApp();
  return (
    <div class="titlebar" data-tauri-drag-region>
      <Mark size={17} />
      <span class="wordmark" data-tauri-drag-region>
        Comfy<i>Vault</i>
      </span>
      <span class="tb-sep" data-tauri-drag-region />
      <span class="tb-path">{app.machine()?.vaultPath ?? ""}</span>
      <button
        class="tb-btn"
        title="Minimize"
        aria-label="Minimize"
        onClick={() => void app.engine.windowMinimize()}
      >
        <Icon name="minus" size={12} />
      </button>
      <button
        class="tb-btn"
        title="Maximize"
        aria-label="Maximize"
        onClick={() => void app.engine.windowToggleMaximize()}
      >
        <Icon name="win" size={12} />
      </button>
      <button
        class="tb-btn x"
        title="Close"
        aria-label="Close"
        onClick={() => void app.engine.windowClose()}
      >
        <Icon name="x" size={12} />
      </button>
    </div>
  );
}

const NAV: ReadonlyArray<{ key: Screen; icon: IconName; title: string }> = [
  { key: "home", icon: "home", title: "Home" },
  { key: "library", icon: "library", title: "Library" },
  { key: "consolidate", icon: "consolidate", title: "Consolidate" },
  { key: "cleanup", icon: "cleanup", title: "Cleanup" },
  { key: "download", icon: "download", title: "Download" },
  { key: "settings", icon: "settings", title: "Settings" },
];

export function Rail() {
  const app = useApp();

  /** The line under the drive meter: what is still to gain, or what was gained. */
  const railNote = (gain: number) => {
    if (!app.hasInstances()) {
      return <div class="rail-sub">nothing registered yet</div>;
    }
    if (app.applyProgress()) {
      return <div class="rail-sub amb">{fmt(gain)} still to come</div>;
    }
    const run = app.lastRun();
    if (run) {
      return <div class="rail-sub grn">{fmt(run.bytesFreed)} freed just now</div>;
    }
    if (gain > 0) {
      return <div class="rail-sub amb">{fmt(gain)} can be freed</div>;
    }
    return <div class="rail-sub">every model is held once already</div>;
  };

  const badgeFor = (key: Screen): number | null => {
    const plan = app.plan();
    if (!plan || !app.hasInstances()) return null;
    if (key === "consolidate") {
      if (app.lastRun()) return null;
      return plan.duplicates.length || null;
    }
    if (key === "cleanup") {
      return plan.aliases.length + plan.orphans.length || null;
    }
    return null;
  };

  const drive = createMemo(() => app.machine()?.vaultDrive ?? null);

  /**
   * The meter shows the drive as it is, and the amber band is the part of it
   * this run gives back. While the run is going the band shrinks in step with
   * it, so the meter and the progress bar tell the same story.
   */
  const meter = createMemo(() => {
    const d = drive();
    const plan = app.plan();
    if (!d) return null;
    const total = d.totalBytes;
    const inFlight = app.applyProgress()?.bytesMoved ?? 0;
    const free = d.freeBytes + inFlight;
    const models = plan ? plan.totals.onDiskBytes - inFlight : 0;
    const gain = plan ? Math.max(plan.totals.reclaimBytes - inFlight, 0) : 0;
    const other = Math.max(total - d.freeBytes - (plan?.totals.onDiskBytes ?? 0), 0);
    const pct = (v: number) => `${((Math.max(v, 0) / total) * 100).toFixed(3)}%`;
    return {
      other: pct(other),
      keep: pct(Math.max(models - gain, 0)),
      gain,
      gainPct: pct(gain),
      free,
      usedPct: usedPercent(total, free),
      total,
    };
  });

  return (
    <div class="rail">
      <nav class="rail-nav">
        <For each={NAV}>
          {(item) => (
            <button
              class="nav"
              classList={{ on: app.screen() === item.key }}
              aria-current={app.screen() === item.key ? "page" : undefined}
              onClick={() => app.actions.go(item.key)}
            >
              <Icon name={item.icon} size={15} />
              <span>{item.title}</span>
              <Show when={item.key === "home" && app.scanProgress()}>
                <span class="pip" title="A scan is running" />
              </Show>
              <Show when={badgeFor(item.key)}>
                {(n) => <span class="badge">{n()}</span>}
              </Show>
            </button>
          )}
        </For>
      </nav>
      <div class="rail-fill" />

      <Show when={app.hasInstances()}>
        <div class="rail-inst">
          <div class="lbl">Installs</div>
          <For each={app.scan()?.instances ?? []}>
            {(instance) => (
              <button
                class="r"
                title={instance.path}
                onClick={() => app.actions.go("settings")}
              >
                <span class="led" classList={{ up: instance.running, idle: !instance.running }} />
                <span class="nm">{instance.name}</span>
                <span class="sz">
                  {fmt(app.plan()?.totals.perInstance.get(instance.id)?.bytes ?? 0)}
                </span>
              </button>
            )}
          </For>
        </div>
      </Show>

      <Show when={meter()}>
        {(m) => (
          <div class="rail-foot">
            <div class="lbl">Drive {drive()?.letter}</div>
            <div class="meter">
              <i class="m-other" style={{ width: m().other }} />
              <i class="m-keep" style={{ width: m().keep }} />
              <Show when={m().gain > 0}>
                <i class="m-gain" style={{ width: m().gainPct }} />
              </Show>
            </div>
            <div class="rail-fig">
              <b>{fmt(m().free)}</b> free
            </div>
            <div class="rail-sub">
              of {fmt(m().total)} &middot; {m().usedPct}% full
            </div>
            {railNote(m().gain)}
          </div>
        )}
      </Show>
    </div>
  );
}

export function Header(props: {
  title: string;
  sub?: string;
  children?: JSX.Element;
}) {
  return (
    <div class="hdr">
      <h1>{props.title}</h1>
      <Show when={props.sub}>
        <span class="sub">{props.sub}</span>
      </Show>
      <span class="sp" />
      {props.children}
    </div>
  );
}

/**
 * The strip that says Apply is held back. It is shown on every screen except
 * the one that explains the reasons, which is Consolidate.
 */
export function Warnbar() {
  const app = useApp();
  const blockers = createMemo(() => {
    const m = app.machine();
    return m ? applyBlockers(m) : [];
  });

  const message = createMemo(() => {
    const list = blockers();
    if (list.length === 2) return "2 things block Apply";
    const first = list[0];
    if (!first) return "";
    return first.kind === "developer_mode_off"
      ? "Symlinks are off in Windows"
      : "ComfyUI is running";
  });

  return (
    <Show when={app.hasInstances() && blockers().length > 0}>
      <div class="warnbar" role="status">
        <Icon name="warn" size={13} />
        <span>{message()}</span>
        <span class="sp" />
        <Show when={app.screen() !== "consolidate"}>
          <button class="btn sm dng" onClick={() => app.actions.go("consolidate")}>
            See what to fix
          </button>
        </Show>
      </div>
    </Show>
  );
}

export function Toaster() {
  const app = useApp();
  return (
    <Show when={app.toast()}>
      {(toast) => (
        <div class="toast" classList={{ bad: toast().tone === "bad" }} role="status" aria-live="polite">
          <Icon name={toast().tone === "bad" ? "warn" : "check"} size={12} />
          <span>{toast().message}</span>
        </div>
      )}
    </Show>
  );
}

/** The shared empty state, for a screen that has nothing to work on yet. */
export function EmptyScreen(props: {
  title: string;
  head: string;
  body: string;
  children?: JSX.Element;
}) {
  return (
    <>
      <Header title={props.title} />
      <div class="screen">
        <div class="empty">
          <span class="gl">
            <Icon name="vault" size={28} />
          </span>
          <h2>{props.head}</h2>
          <p>{props.body}</p>
          <Show when={props.children}>
            <div class="acts">{props.children}</div>
          </Show>
        </div>
      </div>
    </>
  );
}
