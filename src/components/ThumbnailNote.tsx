import { Show, createMemo } from "solid-js";

import { useApp } from "~/state/store";
import type { InstallView } from "~/domain/view";

/** "Studio", "Studio and Sandbox", "A, B and C". */
export function listOf(names: readonly string[]): string {
  if (names.length <= 1) return names[0] ?? "";
  return `${names.slice(0, -1).join(", ")} and ${names[names.length - 1]}`;
}

/**
 * What a person loses by reaching a model through a link: its picture in
 * ComfyUI's model browser, from 0.28.0 on. Loading the model and running a
 * workflow are not affected.
 *
 * An install that does not say which version it runs gets said separately. It
 * is not known to be fine, and saying nothing would read as if it were.
 */
export function ThumbnailNote(props: { where?: "plan" | "plain" }) {
  const app = useApp();
  const affected = createMemo(() =>
    app.installViews().filter((v) => v.thumbnails === "affected"),
  );
  const unknown = createMemo(() =>
    app.installViews().filter((v) => v.thumbnails === "unknown"),
  );
  const labels = (views: readonly InstallView[]) =>
    listOf(views.map((v) => v.install.label));

  return (
    <Show when={affected().length > 0 || unknown().length > 0}>
      <div class="note up">
        <Show when={props.where === "plan"}>One thing does change. </Show>
        <Show when={affected().length > 0}>
          {labels(affected())} {affected().length === 1 ? "runs" : "run"} ComfyUI
          0.28.0 or newer, which will not show a preview thumbnail for a model
          reached through a link, so consolidated models lose their picture in the
          model browser.{" "}
        </Show>
        <Show when={unknown().length > 0}>
          {labels(unknown())} {unknown().length === 1 ? "does" : "do"} not record
          which ComfyUI version {unknown().length === 1 ? "it runs" : "they run"},
          which ComfyUI only began doing in 0.3.11, so this is unknown there.{" "}
        </Show>
        Loading a model and running a workflow are not affected either way.
      </div>
    </Show>
  );
}

/** The same fact, for the one model in front of the person. */
export function ThumbnailNoteForModel(props: { installIds: readonly string[] }) {
  const app = useApp();
  const views = createMemo(() =>
    app
      .installViews()
      .filter(
        (v) =>
          v.thumbnails !== "unaffected" && props.installIds.includes(v.install.id),
      ),
  );
  const anyAffected = () => views().some((v) => v.thumbnails === "affected");

  return (
    <Show when={views().length > 0}>
      <div class="note up">
        <Show
          when={anyAffected()}
          fallback={
            <>
              {listOf(views().map((v) => v.install.label))} does not record which
              ComfyUI version it runs, so whether this model keeps its picture in
              the model browser once it is reached through a link is unknown.
            </>
          }
        >
          Once this model is reached through a link,{" "}
          {listOf(
            views()
              .filter((v) => v.thumbnails === "affected")
              .map((v) => v.install.label),
          )}{" "}
          will not show a picture for it in the model browser.
        </Show>{" "}
        It still loads, and workflows that use it are not affected.
      </div>
    </Show>
  );
}
