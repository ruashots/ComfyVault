import { Show } from "solid-js";

/** Why a link or name button is off while a long job runs. */
export function BusyWait(props: { reason: string | null }) {
  return (
    <Show when={props.reason}>
      {(reason) => <span class="note busy-wait">{reason()}.</span>}
    </Show>
  );
}
