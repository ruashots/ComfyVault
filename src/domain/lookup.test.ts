import { describe, expect, it } from "vitest";

import { LOOKUP_BATCH, LOOKUP_PAUSE_MS, runLookups } from "~/domain/lookup";
import type { ModelMetadata } from "~/ipc/contract";

const hashes = (n: number) => Array.from({ length: n }, (_, i) => i.toString(16).padStart(64, "0"));
const answer = (sha256: string) => ({ sha256, found: false }) as ModelMetadata;

describe("pacing the Civitai lookup", () => {
  it("asks a hundred at a time, one request after another, with a pause between", async () => {
    const events: string[] = [];
    const end = await runLookups({
      hashes: hashes(250),
      fetch: async (batch) => {
        events.push(`ask ${batch.length}`);
        return batch.map(answer);
      },
      sleep: async (ms) => {
        events.push(`pause ${ms}`);
      },
      shouldStop: () => false,
      onAnswers: () => undefined,
    });
    expect(LOOKUP_BATCH).toBe(100);
    expect(events).toEqual([
      "ask 100",
      `pause ${LOOKUP_PAUSE_MS}`,
      "ask 100",
      `pause ${LOOKUP_PAUSE_MS}`,
      "ask 50",
    ]);
    expect(end).toEqual({ kind: "done", asked: 250 });
  });

  it("never has two requests out at once", async () => {
    let out = 0;
    let most = 0;
    await runLookups({
      hashes: hashes(300),
      fetch: async (batch) => {
        out += 1;
        most = Math.max(most, out);
        await Promise.resolve();
        out -= 1;
        return batch.map(answer);
      },
      sleep: async () => undefined,
      shouldStop: () => false,
      onAnswers: () => undefined,
    });
    expect(most).toBe(1);
  });

  it("stops at the first refusal and does not ask again", async () => {
    let calls = 0;
    const end = await runLookups({
      hashes: hashes(300),
      fetch: async (batch) => {
        calls += 1;
        if (calls === 2) throw new Error("slow down");
        return batch.map(answer);
      },
      sleep: async () => undefined,
      shouldStop: () => false,
      onAnswers: () => undefined,
    });
    expect(calls).toBe(2);
    expect(end.kind).toBe("refused");
    expect(end.kind === "refused" && end.asked).toBe(100);
  });

  it("stops before the next request when told to", async () => {
    let calls = 0;
    let stop = false;
    const end = await runLookups({
      hashes: hashes(300),
      fetch: async (batch) => {
        calls += 1;
        stop = true;
        return batch.map(answer);
      },
      sleep: async () => undefined,
      shouldStop: () => stop,
      onAnswers: () => undefined,
    });
    expect(calls).toBe(1);
    expect(end).toEqual({ kind: "stopped" });
  });
});
