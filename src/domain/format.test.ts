import { describe, expect, it } from "vitest";

import {
  agoLong,
  breakPoints,
  clockTime,
  dayAndTime,
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
  timeLeft,
  shortHash,
  startedShort,
  usedPercent,
} from "~/domain/format";
import { MB, SIZE_STRINGS } from "~/test/sizes";

describe("sizes read in the interface's fixed format", () => {
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
  it("prints the sample drive's figure the same way", () => {
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

describe("the person's rule about dashes", () => {
  it("is kept by every file the interface is built from", async () => {
    const { readFileSync, readdirSync, statSync } = await import("node:fs");
    const { join } = await import("node:path");
    const root = join(import.meta.dirname, "..");

    const files: string[] = [];
    const walk = (dir: string) => {
      for (const entry of readdirSync(dir)) {
        const path = join(dir, entry);
        if (statSync(path).isDirectory()) walk(path);
        else if (/\.(ts|tsx|css|html)$/.test(entry)) files.push(path);
      }
    };
    walk(root);
    expect(files.length).toBeGreaterThan(20);

    const offenders: string[] = [];
    for (const file of files) {
      if (file.endsWith("format.test.ts")) continue; // this file names them
      const text = readFileSync(file, "utf8");
      // The character itself, and the three ways to write it without typing it.
      if (/[–—]|&mdash;|&ndash;|\\u201[34]/.test(text)) {
        offenders.push(file.slice(root.length + 1));
      }
    }
    expect(offenders, "an em dash or en dash reached the interface").toEqual([]);
  });
});

describe("when a process started", () => {
  // Built from local parts, so the test reads the same in every time zone.
  const at = (day: number, hour: number, minute: number) =>
    new Date(2026, 8, day, hour, minute).toISOString();
  const now = new Date(2026, 8, 25, 15, 30).getTime();

  it("says today and yesterday by the calendar, not by the last 24 hours", () => {
    expect(startedShort(at(25, 9, 12), now)).toBe("today at 09:12");
    expect(startedShort(at(25, 0, 0), now)).toBe("today at 00:00");
    // 15 hours ago, and still yesterday.
    expect(startedShort(at(24, 23, 59), now)).toBe("yesterday at 23:59");
    expect(startedShort(at(24, 18, 42), now)).toBe("yesterday at 18:42");
    expect(startedShort(at(24, 0, 1), now)).toBe("yesterday at 00:01");
    // Before yesterday it names the day.
    expect(startedShort(at(23, 23, 59), now)).toBe("Wed 23 Sep at 23:59");
    expect(startedShort(at(22, 18, 42), now)).toBe("Tue 22 Sep at 18:42");
  });

  it("writes the long form with the weekday, on a 24-hour clock", () => {
    expect(dayAndTime(at(24, 18, 42))).toBe("Thu 24 Sep at 18:42");
    expect(dayAndTime(at(1, 7, 5))).toBe("Tue 1 Sep at 07:05");
  });

  it("counts minutes under an hour, hours under 48, and days after", () => {
    const back = (ms: number) => new Date(now - ms).toISOString();
    const MIN = 60_000;
    expect(agoLong(back(20 * 1000), now)).toBe("just now");
    expect(agoLong(back(1 * MIN), now)).toBe("1 minute ago");
    expect(agoLong(back(59 * MIN), now)).toBe("59 minutes ago");
    expect(agoLong(back(60 * MIN), now)).toBe("1 hour ago");
    expect(agoLong(back(21 * 60 * MIN), now)).toBe("21 hours ago");
    expect(agoLong(back(47 * 60 * MIN + 59 * MIN), now)).toBe("47 hours ago");
    expect(agoLong(back(48 * 60 * MIN), now)).toBe("2 days ago");
    expect(agoLong(back(10 * 24 * 60 * MIN), now)).toBe("10 days ago");
  });

  it("never prints a negative age when the clocks disagree", () => {
    expect(agoLong(new Date(now + 5 * 60_000).toISOString(), now)).toBe("just now");
  });

  it("reads the engine's own timestamp, whole seconds and a Z", () => {
    // The shape the real engine sent for a process it saw start.
    expect(dayAndTime("2026-09-25T07:36:40Z")).toBe(
      dayAndTime(new Date(Date.UTC(2026, 8, 25, 7, 36, 40)).toISOString()),
    );
    expect(agoLong("2026-09-25T07:36:40Z", Date.UTC(2026, 8, 25, 9, 0, 0))).toBe(
      "1 hour ago",
    );
  });
});

describe("how long a download has left", () => {
  it("says minutes, then hours, in words", () => {
    expect(timeLeft(300)).toBe("about 5 minutes left");
    expect(timeLeft(61)).toBe("about 1 minute left");
    expect(timeLeft(59 * 60)).toBe("about 59 minutes left");
    expect(timeLeft(2 * 3600)).toBe("about 2 hours left");
    expect(timeLeft(3600)).toBe("about 1 hour left");
    expect(timeLeft(20)).toBe("less than a minute left");
  });

  it("says it is working until the speed is known", () => {
    expect(timeLeft(null)).toBe("working");
    expect(timeLeft(Number.NaN)).toBe("working");
    expect(timeLeft(Number.POSITIVE_INFINITY)).toBe("working");
  });
});
