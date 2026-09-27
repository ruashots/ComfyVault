import { For, Show } from "solid-js";

import { Icon } from "~/components/Icon";
import { countOf } from "~/domain/format";
import { openConfirm } from "~/modals/confirm";
import { useApp } from "~/state/store";
import type { LinkRecord } from "~/ipc/contract";

/**
 * Links that point at a file that is not there.
 *
 * This is the worst thing that can be wrong with a vault, so it is said before
 * anything else on the screen. ComfyUI lists a broken link in its model
 * dropdown and then fails to load it, and a custom node that re-downloads the
 * model it thinks is missing writes straight through the broken link and puts
 * the file inside the vault, where nothing is expecting it.
 */
export function DanglingLinks() {
  const app = useApp();
  const links = () => app.danglingLinks();

  const removeOne = (link: LinkRecord) => {
    void app.actions.run(
      () => app.engine.removeLink(link.id),
      `Removed the broken link at ${link.absPath}`,
    );
  };

  const removeAll = () => {
    const all = [...links()];
    openConfirm(app, {
      title: "Remove every broken link",
      cta: `Remove ${countOf(all.length, "link", "links")}`,
      body: [
        [
          { text: countOf(all.length, "link", "links"), emph: true },
          {
            text: `${all.length === 1 ? " is removed from the install it sits in" : " are removed from the installs they sit in"}. Nothing in the vault is touched, and no model file is deleted: a broken link points at nothing, so there is nothing to lose.`,
          },
        ],
        [
          {
            text: "ComfyUI will stop offering these models until the file they pointed at is back. That is better than offering one that fails to load.",
          },
        ],
      ],
      action: async () => {
        for (const link of all) await app.engine.removeLink(link.id);
      },
    });
  };

  return (
    <Show when={links().length > 0}>
      <div class="blk" role="alert">
        <h3>
          <Icon name="warn" size={13} />
          {links().length}{" "}
          {links().length === 1 ? "link points" : "links point"} at a file that is
          not there
        </h3>
        <div class="blkrow">
          <div class="bl">
            <div class="bt">Remove these before you do anything else</div>
            <div class="bd">
              ComfyUI will list each of these in its model dropdown and then fail
              to load it. Worse, a custom node that re-downloads the model it
              thinks is missing will write straight through the broken link and
              put the file inside the vault. Removing the link costs nothing: it
              points at nothing.
            </div>
          </div>
          <div class="ba">
            <button class="btn sm dng" onClick={removeAll}>
              <Icon name="trash" size={11} />
              Remove {links().length === 1 ? "it" : "them all"}
            </button>
          </div>
        </div>
        <For each={links().slice(0, 8)}>
          {(link) => (
            <div class="blkrow">
              <div class="bl">
                <div class="bt">{link.linkName}</div>
                <div class="bd">{link.absPath}</div>
              </div>
              <div class="ba">
                <button class="btn sm" onClick={() => removeOne(link)}>
                  Remove this one
                </button>
              </div>
            </div>
          )}
        </For>
        <Show when={links().length > 8}>
          <div class="scope">
            {links().length - 8} more are in the same state.
          </div>
        </Show>
      </div>
    </Show>
  );
}

/**
 * A real file sitting where a link belonged. Nothing is broken, but the vault's
 * record is out of date and the space that link was saving is being used again.
 */
export function ReplacedLinks() {
  const app = useApp();
  const links = () => app.health()?.replacedLinks ?? [];

  return (
    <Show when={links().length > 0}>
      <div class="note up">
        {links().length} {links().length === 1 ? "path holds" : "paths hold"} a
        real file where a link into the vault used to be. Something wrote over
        the link, so that model now takes room in both places. A scan picks them
        up again.
      </div>
    </Show>
  );
}
