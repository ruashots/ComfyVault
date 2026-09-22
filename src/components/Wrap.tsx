import { For, createMemo } from "solid-js";
import { breakPoints } from "~/domain/format";

/**
 * A long path or filename that is allowed to wrap, but only where a Windows
 * path or a model filename has a seam: a separator, an underscore, a dot or a
 * dash. It never breaks in the middle of a word.
 */
export function Wrap(props: { text: string }) {
  const pieces = createMemo(() => breakPoints(props.text));
  return (
    <For each={pieces()}>
      {(piece, i) => (
        <>
          {piece}
          {i() < pieces().length - 1 ? <wbr /> : null}
        </>
      )}
    </For>
  );
}
