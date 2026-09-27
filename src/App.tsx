import { Match, Show, Switch, onCleanup, onMount } from "solid-js";

import { Icon } from "~/components/Icon";
import { Boundary } from "~/components/Boundary";
import { NAV, Rail, Titlebar, Toaster } from "~/components/Shell";
import { ConfirmModalView } from "~/modals/confirm";
import { LinkFolderView } from "~/modals/linkfolder";
import { PickerModalView } from "~/modals/picker";
import { CleanupScreen } from "~/screens/Cleanup";
import { ConsolidateScreen } from "~/screens/Consolidate";
import { DownloadScreen } from "~/screens/Download";
import { HomeScreen } from "~/screens/Home";
import { LibraryScreen } from "~/screens/Library";
import { SettingsScreen } from "~/screens/Settings";
import { useApp } from "~/state/store";

export function App() {
  const app = useApp();

  /** Esc closes whatever is on top: a modal, a menu, the drawer. */
  const onKeyDown = (event: KeyboardEvent) => {
    if (event.key !== "Escape") return;
    if (app.modal()) app.setModal(null);
    else if (app.categoryMenuOpen()) app.actions.setCategoryMenuOpen(false);
    else if (app.lib.drawerOpen && app.screen() === "library") {
      app.setLib("drawerOpen", false);
    }
  };

  /** A click anywhere else closes the folder menu. */
  const onPointerDown = (event: MouseEvent) => {
    if (!app.categoryMenuOpen()) return;
    if ((event.target as HTMLElement | null)?.closest(".menu-wrap")) return;
    app.actions.setCategoryMenuOpen(false);
  };

  onMount(() => {
    window.addEventListener("keydown", onKeyDown);
    window.addEventListener("pointerdown", onPointerDown);
    onCleanup(() => {
      window.removeEventListener("keydown", onKeyDown);
      window.removeEventListener("pointerdown", onPointerDown);
    });
  });

  return (
    <div class="win">
      <Titlebar />
      <div class="body">
        <Rail />
        <div class="main">
          <Show when={app.ready()} fallback={<Starting />}>
            <Show when={!app.failure()} fallback={<Unreachable />}>
              <Boundary where={screenTitle(app.screen())}>
              <Switch fallback={<HomeScreen />}>
                <Match when={app.screen() === "home"}>
                  <HomeScreen />
                </Match>
                <Match when={app.screen() === "library"}>
                  <LibraryScreen />
                </Match>
                <Match when={app.screen() === "consolidate"}>
                  <ConsolidateScreen />
                </Match>
                <Match when={app.screen() === "cleanup"}>
                  <CleanupScreen />
                </Match>
                <Match when={app.screen() === "download"}>
                  <DownloadScreen />
                </Match>
                <Match when={app.screen() === "settings"}>
                  <SettingsScreen />
                </Match>
              </Switch>
              </Boundary>
            </Show>
          </Show>
        </div>
      </div>
      <PickerModalView />
      <ConfirmModalView />
      <LinkFolderView />
      <Toaster />
    </div>
  );
}

/** The name the rail gives a screen, for when that screen cannot draw itself. */
function screenTitle(screen: string): string {
  return NAV.find((item) => item.key === screen)?.title ?? "This screen";
}

/** The half second before the engine has answered. */
function Starting() {
  return (
    <div class="screen">
      <div class="empty">
        <div class="lbl">Reading the vault</div>
      </div>
    </div>
  );
}

/** The engine did not answer, and the window says so rather than sitting blank. */
function Unreachable() {
  const app = useApp();
  return (
    <div class="screen">
      <div class="empty">
        <span class="gl">
          <Icon name="warn" size={28} />
        </span>
        <h2>ComfyVault could not read the vault</h2>
        <p>
          The part of ComfyVault that reads your drive answered with a problem.
          Nothing has been changed. This is what it said.
        </p>
        <div class="readout">{app.failure()}</div>
        <div class="acts">
          <button class="btn pri" onClick={() => void app.actions.refresh()}>
            <Icon name="refresh" size={13} />
            Try again
          </button>
        </div>
      </div>
    </div>
  );
}
