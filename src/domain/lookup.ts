/**
 * How the Civitai lookup paces itself.
 *
 * Civitai is a public service with rate limits, and this is an optional
 * feature, so the lookup asks gently: one request at a time, a hundred hashes
 * per request (the most one request takes), and a pause between requests. The
 * first refusal stops the pass. A refusal is either the network being down or
 * Civitai asking the app to slow down, and in both cases asking again at once
 * is the wrong answer.
 */

import type { ModelMetadata } from "~/ipc/contract";

/** The most hashes one request to Civitai takes. */
export const LOOKUP_BATCH = 100;

/** The pause between two requests. */
export const LOOKUP_PAUSE_MS = 2000;

export type LookupEnd =
  | { kind: "done"; asked: number }
  | { kind: "stopped" }
  | { kind: "refused"; asked: number; error: unknown };

export async function runLookups(options: {
  hashes: readonly string[];
  fetch: (batch: string[]) => Promise<ModelMetadata[]>;
  sleep: (ms: number) => Promise<void>;
  /** Checked before every request: the switch went off, or the screen closed. */
  shouldStop: () => boolean;
  onAnswers: (answers: ModelMetadata[], asked: number) => void;
}): Promise<LookupEnd> {
  let asked = 0;
  for (let start = 0; start < options.hashes.length; start += LOOKUP_BATCH) {
    if (start > 0) await options.sleep(LOOKUP_PAUSE_MS);
    if (options.shouldStop()) return { kind: "stopped" };
    const batch = options.hashes.slice(start, start + LOOKUP_BATCH);
    try {
      const answers = await options.fetch(batch);
      asked += batch.length;
      options.onAnswers(answers, asked);
    } catch (error) {
      return { kind: "refused", asked, error };
    }
  }
  return { kind: "done", asked };
}
