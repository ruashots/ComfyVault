import { ErrorBoundary, type JSX } from "solid-js";

import { Icon } from "~/components/Icon";

/**
 * A panel that cannot draw itself says so, in front of the person, instead of
 * leaving an empty heading behind.
 *
 * This program moves the only copy someone has of their models. An area that
 * quietly shows nothing is worse than an error: it reads as "there is nothing
 * here", and a person acts on that. So the failure is stated, the rest of the
 * screen keeps working, and the one thing they need to know first is said
 * first: nothing on the drive has been touched by a drawing failure.
 */
export function Boundary(props: {
  /** The panel's own heading, so the person can see which part failed. */
  where: string;
  children: JSX.Element;
}) {
  return (
    <ErrorBoundary
      fallback={(error, reset) => (
        <div class="bnd" role="alert">
          <h4>
            <Icon name="warn" size={12} />
            ComfyVault could not draw this part of the screen
          </h4>
          <p>
            "{props.where}" is not showing. The rest of this screen still works.
            Nothing on your drive has been changed by this. This is what went
            wrong.
          </p>
          <div class="readout">{messageOf(error)}</div>
          <button class="btn sm" onClick={reset}>
            <Icon name="refresh" size={11} />
            Draw it again
          </button>
        </div>
      )}
    >
      {props.children}
    </ErrorBoundary>
  );
}

function messageOf(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === "string") return error;
  return String(error);
}
