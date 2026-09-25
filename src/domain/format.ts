/**
 * Every number the interface prints goes through here, so one size reads the
 * same on every screen. The thresholds and the number of decimals come from
 * design/mock/comfyvault.html and must not drift.
 */

const MB = 1024 * 1024;
const GB = 1024 * MB;
const TB = 1024 * GB;

/** "1.09 TB", "16 GB", "1.3 GB", "241 MB" */
export function fmt(bytes: number): string {
  return `${fmtN(bytes)} ${fmtU(bytes)}`;
}

/** The number on its own, for the places that style the unit differently. */
export function fmtN(bytes: number): string {
  if (bytes >= TB) return (bytes / TB).toFixed(2);
  if (bytes >= 10 * GB) return String(Math.round(bytes / GB));
  if (bytes >= GB) return (bytes / GB).toFixed(1);
  return String(Math.round(bytes / MB));
}

/** The unit on its own. */
export function fmtU(bytes: number): string {
  if (bytes >= TB) return "TB";
  if (bytes >= GB) return "GB";
  return "MB";
}

/** Whole megabytes with thousands separators, for the exact size in the drawer. */
export function fmtExactMB(bytes: number): string {
  return `${Math.round(bytes / MB).toLocaleString("en-US")} MB`;
}

/** A hash shown as its first and last eight characters. */
export function shortHash(sha: string): string {
  if (sha.length <= 17) return sha;
  return `${sha.slice(0, 8)}\u2026${sha.slice(-8)}`;
}

/** Trim the middle out of a long filename so both ends stay readable. */
export function mid(s: string, max: number): string {
  if (s.length <= max) return s;
  const keep = max - 1;
  const head = Math.ceil(keep * 0.6);
  const tail = keep - head;
  return `${s.slice(0, head)}\u2026${s.slice(s.length - tail)}`;
}

/** "56%" of the drive used, given what is free. */
export function usedPercent(totalBytes: number, freeBytes: number): number {
  if (totalBytes <= 0) return 0;
  return Math.round((1 - freeBytes / totalBytes) * 100);
}

/**
 * Split a string so it can wrap only at the separators a Windows path and a
 * model filename actually use, never in the middle of a word. The caller puts a
 * <wbr> between the pieces.
 */
export function breakPoints(s: string): string[] {
  const out: string[] = [];
  let buf = "";
  for (const ch of s) {
    buf += ch;
    if (ch === "\\" || ch === "/" || ch === "_" || ch === "." || ch === "-") {
      out.push(buf);
      buf = "";
    }
  }
  if (buf) out.push(buf);
  return out;
}

/** How long a scan has left, in the unit that still means something. */
export function minutesLeft(seconds: number | null): string {
  // A missing estimate and an unusable one are the same fact: it is not known.
  // Saying "about NaN min left" is worse than saying nothing.
  if (!Number.isFinite(seconds as number)) return "working";
  const left = seconds as number;
  if (left < 10) return "finishing";
  if (left < 60) return `about ${Math.round(left / 5) * 5} seconds left`;
  return `about ${Math.round(left / 60)} min left`;
}

/** "about 41 seconds left" / "nearly done" for a run that takes seconds. */
export function secondsLeft(seconds: number | null, overall: number): string {
  if (!Number.isFinite(seconds as number)) return "working";
  const left = seconds as number;
  if (overall >= 0.97 || left <= 0) return "nearly done";
  return `about ${Math.round(left)} seconds left`;
}

const MONTHS = [
  "Jan", "Feb", "Mar", "Apr", "May", "Jun",
  "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/**
 * How long ago something happened, said the way the person would say it:
 * minutes and hours while it is today, a date after that.
 */
export function relativeTime(iso: string, now: number = Date.now()): string {
  const then = new Date(iso).getTime();
  if (Number.isNaN(then)) return iso;
  const seconds = Math.max(0, Math.round((now - then) / 1000));
  if (seconds < 60) return "just now";
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  return dayMonth(iso);
}

/** "14 Sep" */
export function dayMonth(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  return `${d.getDate()} ${MONTHS[d.getMonth()]}`;
}

/** "13:47:21" from an ISO timestamp, for the moves.log lines. */
export function clockTime(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  const p = (n: number) => String(n).padStart(2, "0");
  return `${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`;
}

/** The drive letter a Windows path sits on, "C:" for C:\ComfyVault. */
export function driveOf(path: string): string {
  return path.slice(0, 2).toUpperCase();
}

/** The last segment of a Windows path, for "inside Downloads". */
export function leafOf(path: string): string {
  const trimmed = path.endsWith("\\") ? path.slice(0, -1) : path;
  const i = trimmed.lastIndexOf("\\");
  return i < 0 ? trimmed : trimmed.slice(i + 1);
}

/** Join a Windows folder and a name without doubling the separator. */
export function joinPath(parent: string, name: string): string {
  return parent.endsWith("\\") ? parent + name : `${parent}\\${name}`;
}

const WEEKDAYS = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

/** "18:42", local time, 24-hour clock. */
function hourMinute(d: Date): string {
  const p = (n: number) => String(n).padStart(2, "0");
  return `${p(d.getHours())}:${p(d.getMinutes())}`;
}

/** "Thu 24 Sep at 18:42", in local time. */
export function dayAndTime(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  return `${WEEKDAYS[d.getDay()]} ${d.getDate()} ${MONTHS[d.getMonth()]} at ${hourMinute(d)}`;
}

/**
 * When a process started, the short way: "today at 09:12", "yesterday at
 * 18:42", and the day itself before that. Today and yesterday are calendar
 * days in local time, not the last 24 hours.
 */
export function startedShort(iso: string, now: number = Date.now()): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  const today = new Date(now);
  const midnight = new Date(today.getFullYear(), today.getMonth(), today.getDate());
  const dayBefore = new Date(today.getFullYear(), today.getMonth(), today.getDate() - 1);
  if (d >= midnight) return `today at ${hourMinute(d)}`;
  if (d >= dayBefore) return `yesterday at ${hourMinute(d)}`;
  return dayAndTime(iso);
}

/** "21 hours ago": minutes under an hour, hours under 48, days after that. */
export function agoLong(iso: string, now: number = Date.now()): string {
  const then = new Date(iso).getTime();
  if (Number.isNaN(then)) return iso;
  const minutes = Math.floor(Math.max(0, now - then) / 60_000);
  const say = (n: number, unit: string) => `${n} ${unit}${n === 1 ? "" : "s"} ago`;
  if (minutes < 1) return "just now";
  if (minutes < 60) return say(minutes, "minute");
  const hours = Math.floor(minutes / 60);
  if (hours < 48) return say(hours, "hour");
  return say(Math.floor(hours / 24), "day");
}
