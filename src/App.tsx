import { Match, Show, Switch, onCleanup, onMount } from "solid-js";

import { Rail, Titlebar, Toaster } from "~/components/Shell";
import { ConfirmModalView } from "~/modals/confirm";
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

  /** Esc closes whatever is on top: a modal, a menu, the drawer, a rename. */
  const onKeyDown = (event: KeyboardEvent) => {
    if (event.key !== "Escape") return;
    if (app.modal()) {
      app.setModal(null);
    } else if (app.folderMenuOpen()) {
      app.actions.setFolderMenuOpen(false);
    } else if (app.lib.drawerOpen && app.screen() === "library") {
      app.setLib("drawerOpen", false);
    } else if (app.renaming()) {
      app.actions.cancelRename();
    }
  };

  /** A click anywhere else closes the folder menu. */
  const onPointerDown = (event: MouseEvent) => {
    if (!app.folderMenuOpen()) return;
    const target = event.target as HTMLElement | null;
    if (target?.closest(".menu-wrap")) return;
    app.actions.setFolderMenuOpen(false);
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
          </Show>
        </div>
      </div>
      <PickerModalView />
      <ConfirmModalView />
      <Toaster />
    </div>
  );
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
