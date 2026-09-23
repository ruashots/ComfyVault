import { describe, expect, it } from "vitest";

import {
  breakPoints,
  clockTime,
  dayMonth,
  driveOf,
  fmt,
  fmtExactMB,
  fmtN,
  fmtU,
  joinPath,
  leafOf,
  mid,
  minutesLeft,
  relativeTime,
  secondsLeft,
  shortHash,
  usedPercent,
} from "~/domain/format";
import { MB, SIZE_STRINGS } from "~/test/blessed";

describe("sizes read the way the blessed mock reads them", () => {
  it.each(SIZE_STRINGS.map((row) => [...row] as [number, string]))(
    "%i MB reads as %s",
    (mb: number, expected: string) => {
      expect(fmt(mb * MB)).toBe(expected);
    },
  );

  it("splits the number from the unit for the big figures", () => {
    expect(fmtN(614673 * MB)).toBe("600");
    expect(fmtU(614673 * MB)).toBe("GB");
    expect(fmtN(1542224 * MB)).toBe("1.47");
    expect(fmtU(1542224 * MB)).toBe("TB");
  });

  it("switches unit exactly at the boundary, never before", () => {
    expect(fmt(1023 * MB)).toBe("1023 MB");
    expect(fmt(1024 * MB)).toBe("1.0 GB");
    expect(fmt(10239 * MB)).toBe("10.0 GB");
    expect(fmt(10240 * MB)).toBe("10 GB");
    expect(fmt(1048575 * MB)).toBe("1024 GB");
    expect(fmt(1048576 * MB)).toBe("1.00 TB");
  });

  it("says zero rather than nothing", () => {
    expect(fmt(0)).toBe("0 MB");
  });

  it("gives the exact megabytes with separators for the drawer", () => {
    expect(fmtExactMB(16793 * MB)).toBe("16,793 MB");
    expect(fmtExactMB(241 * MB)).toBe("241 MB");
  });
});

describe("what percentage of the drive is used", () => {
  it("matches the mock's figure for the person's drive", () => {
    expect(usedPercent(1908408 * MB, 139264 * MB)).toBe(93);
  });

  it("falls as space comes back", () => {
    expect(usedPercent(1908408 * MB, (139264 + 614673) * MB)).toBe(60);
  });

  it("does not divide by a drive of no size", () => {
    expect(usedPercent(0, 0)).toBe(0);
  });
});

describe("long names and paths", () => {
  it("leaves a short name alone", () => {
    expect(mid("clip_l.safetensors", 72)).toBe("clip_l.safetensors");
  });

  it("takes the middle out of a long one and keeps both ends", () => {
    const name = "Wan2.1_I2V_14B_lightx2v_cfg_step_distill_lora_rank64.safetensors";
    const short = mid(name, 34);
    expect(short).toHaveLength(34);
    expect(short).toContain("\u2026");
    expect(short.startsWith("Wan2.1_I2V")).toBe(true);
    expect(short.endsWith("safetensors")).toBe(true);
  });

  it("only offers to wrap where a path or a filename has a seam", () => {
    expect(breakPoints("C:\\ComfyUI\\models\\vae")).toEqual([
      "C:\\",
      "ComfyUI\\",
      "models\\",
      "vae",
    ]);
    expect(breakPoints("wan_2.1_vae.safetensors")).toEqual([
      "wan_",
      "2.",
      "1_",
      "vae.",
      "safetensors",
    ]);
  });

  it("shortens a hash to its two ends", () => {
    const sha = "a".repeat(32) + "b".repeat(32);
    expect(shortHash(sha)).toBe("aaaaaaaa\u2026bbbbbbbb");
    expect(shortHash("short")).toBe("short");
  });
});

describe("Windows paths", () => {
  it("reads the drive off a path", () => {
    expect(driveOf("C:\\ComfyVault")).toBe("C:");
    expect(driveOf("d:\\ai-models\\ltx\\")).toBe("D:");
  });

  it("reads the last folder off a path, with or without a trailing slash", () => {
    expect(leafOf("C:\\Users\\alex\\Downloads")).toBe("Downloads");
    expect(leafOf("C:\\Users\\alex\\Downloads\\")).toBe("Downloads");
    expect(leafOf("C:\\")).toBe("C:");
  });

  it("joins without doubling the separator", () => {
    expect(joinPath("C:\\", "ComfyVault")).toBe("C:\\ComfyVault");
    expect(joinPath("C:\\Users", "alex")).toBe("C:\\Users\\alex");
  });
});

describe("time", () => {
  const now = new Date("2026-09-22T15:00:00.000Z").getTime();

  it("says how long ago the scan was", () => {
    expect(relativeTime("2026-09-22T14:59:30.000Z", now)).toBe("just now");
    expect(relativeTime("2026-09-22T14:45:00.000Z", now)).toBe("15m ago");
    expect(relativeTime("2026-09-22T13:00:00.000Z", now)).toBe("2h ago");
  });

  it("gives a date once it is no longer today", () => {
    expect(relativeTime("2026-09-14T09:13:00.000Z", now)).toBe(
      dayMonth("2026-09-14T09:13:00.000Z"),
    );
  });

  it("hands back anything it cannot read, rather than printing rubbish", () => {
    expect(relativeTime("not a date", now)).toBe("not a date");
    expect(dayMonth("not a date")).toBe("not a date");
    expect(clockTime("not a date")).toBe("not a date");
  });

  it("counts a scan down in the unit that still means something", () => {
    expect(minutesLeft(240)).toBe("about 4 min left");
    expect(minutesLeft(45)).toBe("about 45 seconds left");
    expect(minutesLeft(9)).toBe("finishing");
    expect(minutesLeft(0)).toBe("finishing");
    expect(minutesLeft(null)).toBe("working");
    expect(secondsLeft(41, 0.5)).toBe("about 41 seconds left");
    expect(secondsLeft(1, 0.99)).toBe("nearly done");
    expect(secondsLeft(0, 0.2)).toBe("nearly done");
    // No estimate is not "nearly done". The interface does not claim a run is
    // almost over when nobody told it how long is left.
    expect(secondsLeft(null, 0.2)).toBe("working");
  });
});
