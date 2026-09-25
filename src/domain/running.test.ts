import { describe, expect, it } from "vitest";

import { holdsFact, noModelFileKnown } from "~/domain/running";
import type { RunningComfy } from "~/ipc/contract";

const proc = (holdsModelFiles: boolean | null): RunningComfy => ({
  pid: 18244,
  name: "python.exe",
  exePath: null,
  cwd: null,
  commandLine: [],
  matchedInstallIds: ["a"],
  matchReason: "cwdUnderRoot",
  startedAt: null,
  listeningPorts: null,
  holdsModelFiles,
});

const scan = (filesSeen: number, cancelled = false) => ({ cancelled, totals: { filesSeen } });

describe("whether a running ComfyUI holds a model file", () => {
  it("says there is nothing to check when no model file is known", () => {
    expect(holdsFact(proc(null), true)).toEqual({
      value: "nothing to check yet",
      note: "ComfyVault knows no model file until a scan finds one",
    });
  });

  it("leaves a null to mean Windows did not say when files were there to ask about", () => {
    expect(holdsFact(proc(null), false)).toBeNull();
  });

  it("reports Windows' own answer whatever else is known", () => {
    expect(holdsFact(proc(true), true)).toEqual({ value: "holds some open" });
    expect(holdsFact(proc(false), false)).toEqual({ value: "holds none open right now" });
  });
});

describe("when the engine knows no model file", () => {
  // The engine asks about every file the last scan found and every file in
  // the vault, and asks nothing when there are none.
  it("is before any scan, with an empty vault", () => {
    expect(noModelFileKnown(null, 0)).toBe(true);
  });

  it("is after a scan that found nothing, with an empty vault", () => {
    expect(noModelFileKnown(scan(0), 0)).toBe(true);
  });

  it("is not once the vault holds a file", () => {
    expect(noModelFileKnown(null, 3)).toBe(false);
  });

  it("is not once a scan found files", () => {
    expect(noModelFileKnown(scan(12), 0)).toBe(false);
  });

  it("is not after a cancelled scan, which may have listed files before it stopped", () => {
    expect(noModelFileKnown(scan(0, true), 0)).toBe(false);
  });

  it("is not when the vault's size is not known", () => {
    expect(noModelFileKnown(null, null)).toBe(false);
  });
});
