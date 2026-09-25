import { For, Show, createMemo, type JSX } from "solid-js";

import { Icon, Mark, type IconName } from "~/components/Icon";
import { driveKindShort, isReadable, volumeLabel } from "~/domain/drives";
import { fmt, usedPercent } from "~/domain/format";
import { warnbarRunning } from "~/domain/running";
import { gateBlockers } from "~/domain/selection";
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
      <span class="tb-path">
        {app.vault()?.root ?? "no vault folder yet"}
      </span>
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

export const NAV: ReadonlyArray<{ key: Screen; icon: IconName; title: string }> = [
  { key: "home", icon: "home", title: "Home" },
  { key: "library", icon: "library", title: "Library" },
  { key: "consolidate", icon: "consolidate", title: "Consolidate" },
  { key: "cleanup", icon: "cleanup", title: "Cleanup" },
  { key: "download", icon: "download", title: "Download" },
  { key: "settings", icon: "settings", title: "Settings" },
];

export function Rail() {
  const app = useApp();

  const badgeFor = (key: Screen): number | null => {
    if (!app.hasInstalls()) return null;
    if (key === "consolidate") {
      if (app.runOnScreen()) return null;
      return app.planView()?.duplicates.length || null;
    }
    if (key === "cleanup") {
      return (
        app.danglingLinks().length +
          app.nameGroups().length +
          app.orphans().length || null
      );
    }
    return null;
  };

  /**
   * The meter shows the drive as it is, and the amber band is the part of it
   * this run gives back. While the run is going the band shrinks in step with
   * it, so the meter and the progress bar tell the same story.
   */
  const meter = createMemo(() => {
    const drive = app.vault();
    // A drive that could not be read has no meter to draw. Its two figures
    // arrive null together, and drawing zero of zero would show a full drive.
    if (!drive || drive.totalBytes === null || drive.freeBytes === null) return null;
    const total = drive.totalBytes;
    const inFlight = app.applyProgress()?.bytesFreed ?? 0;
    const free = drive.freeBytes;
    const models = app.scan()?.totals.movableBytes ?? 0;
    const gain = Math.max((app.plan()?.totals.bytesFreed ?? 0) - inFlight, 0);
    const other = Math.max(total - free - models, 0);
    const pct = (v: number) => `${((Math.max(v, 0) / total) * 100).toFixed(3)}%`;
    return {
      other: pct(other),
      keep: pct(Math.max(models - gain, 0)),
      gain,
      gainPct: pct(gain),
      free,
      usedPct: usedPercent(total, free),
      total,
      volume: volumeLabel(drive.volume),
    };
  });

  /** The line under the drive meter: what is still to gain, or what was gained. */
  const railNote = (gain: number) => {
    if (!app.hasInstalls()) {
      return <div class="rail-sub">nothing registered yet</div>;
    }
    if (app.applyProgress()) {
      // What the run has actually freed, read from the run. "Still to come"
      // was a prediction minus a measurement, and when the measurement ran
      // ahead of the prediction it clamped to zero while files remained.
      const freed = app.applyProgress()!.bytesFreed;
      return freed > 0 ? <div class="rail-sub amb">{fmt(freed)} freed so far</div> : null;
    }
    // The run is still on record while it is undone, and "freed just now"
    // beside an undo putting those bytes back says the opposite.
    if (app.revertProgress()) {
      return <div class="rail-sub">putting files back</div>;
    }
    const run = app.runOnScreen();
    if (app.cutOffRun()) {
      return <div class="rail-sub">a run stopped part way</div>;
    }
    if (run?.state === "partlyReverted") {
      return <div class="rail-sub">undo stopped part way</div>;
    }
    if (run) {
      return <div class="rail-sub grn">{fmt(run.bytesFreed)} freed just now</div>;
    }
    if (gain > 0) {
      return <div class="rail-sub amb">{fmt(gain)} can be freed</div>;
    }
    // Before a scan there is no answer, and "every model is held once" would
    // be a claim about files nobody has read.
    if (app.nothingRead()) {
      return <div class="rail-sub">nothing read yet</div>;
    }
    if (app.scanPredatesUndo()) {
      return <div class="rail-sub">not scanned since the undo</div>;
    }
    return <div class="rail-sub">every model is held once already</div>;
  };

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

      {/* Registered ones only. An install still waiting for a vault has
          nothing to report yet, and a heading over nothing says less than no
          heading at all. */}
      <Show when={app.installViews().length > 0}>
        <div class="rail-inst">
          <div class="lbl">Installs</div>
          <For each={app.installViews()}>
            {(view) => (
              <button
                class="r"
                title={view.install.root}
                onClick={() => app.actions.go("settings")}
              >
                <span class="led" classList={{ up: view.running, idle: !view.running }} />
                <span class="nm">{view.install.label}</span>
                <span class="sz">{app.nothingRead() ? "" : fmt(view.bytes)}</span>
              </button>
            )}
          </For>
        </div>
      </Show>

      {/* Before a vault folder exists there is no drive to instrument, but the
          drives themselves are known. Listing them is what makes the choice the
          setup screen is asking for a good one. */}
      <Show when={!app.hasVault()}>
        <div class="rail-foot">
          <div class="lbl">Drives</div>
          <For each={app.drives()}>
            {(drive) => (
              <>
                <div class="rail-drive">
                  <span class="dl">{drive.root.replace(/\\+$/, "")}</span>
                  <span class="dv">
                    <Show
                      when={isReadable(drive)}
                      fallback={
                        <span style={{ color: "var(--t-dim)" }}>not readable</span>
                      }
                    >
                      <b>{fmt(drive.freeBytes!)}</b> free
                    </Show>
                  </span>
                </div>
                <Show when={isReadable(drive)}>
                  <div class="rail-drive-sub">of {fmt(drive.totalBytes!)}</div>
                </Show>
                {/* The kind matters here, because a vault on a drive that can
                    go away takes every install's models with it. */}
                <Show when={driveKindShort(drive.kind)}>
                  {(word) => (
                    <div
                      class="rail-drive-sub"
                      style={{ color: "var(--t-muted)" }}
                    >
                      {word()}
                    </div>
                  )}
                </Show>
              </>
            )}
          </For>
        </div>
      </Show>

      <Show when={meter()}>
        {(m) => (
          <div class="rail-foot">
            <div class="lbl">Drive {m().volume}</div>
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
 * The strip that says Apply is held back. It names every thing in the way, each
 * with the facts a person can check for themselves, and sends them to the
 * screen that explains what to do. Too long for the window, it ends in an
 * ellipsis.
 */
export function Warnbar() {
  const app = useApp();
  const blockers = () => gateBlockers(app.gate());

  const parts = createMemo((): { lead: string; detail: string }[] => {
    const dangling = app.danglingLinks().length;
    if (dangling > 0) {
      return [
        {
          lead: `${dangling} ${dangling === 1 ? "link points" : "links point"} at a file that is not there`,
          detail: "",
        },
      ];
    }
    return blockers().map((blocker) => {
      switch (blocker.kind) {
        case "interrupted_apply":
          return { lead: "A run stopped part way through", detail: "" };
        case "symlinks_unsupported":
          return { lead: "Windows will not let this app create links", detail: "" };
        case "comfy_running":
          return warnbarRunning(blocker.processes, app.installs());
      }
    });
  });
  const message = () =>
    parts()
      .map((p) => p.lead + p.detail)
      .join(" · ");

  const showing = () =>
    app.hasInstalls() && (blockers().length > 0 || app.danglingLinks().length > 0);
  /** A broken link is settled in Cleanup; everything else in Consolidate. */
  const goesTo = () => (app.danglingLinks().length > 0 ? "cleanup" : "consolidate");

  return (
    <Show when={showing()}>
      <div class="warnbar" role="status">
        <Icon name="warn" size={13} />
        <span class="wt" title={message()}>
          <For each={parts()}>
            {(part, i) => (
              <>
                <Show when={i() > 0}>
                  <i> &middot; </i>
                </Show>
                <b>{part.lead}</b>
                <Show when={part.detail}>
                  <i>{part.detail}</i>
                </Show>
              </>
            )}
          </For>
        </span>
        <span class="sp" />
        <Show when={app.screen() !== goesTo()}>
          <button class="btn sm dng" onClick={() => app.actions.go(goesTo())}>
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
        <div
          class="toast"
          classList={{ bad: toast().tone === "bad" }}
          role="status"
          aria-live="polite"
        >
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
