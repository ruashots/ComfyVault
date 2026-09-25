/**
 * How a running ComfyUI is described. Every fact here is one the engine read
 * from Windows, and every one of them is something the person can find again
 * in Task Manager. Where Windows gave no answer, the words say so and nothing
 * is guessed in its place.
 */

import type { Engine, Install, RunningComfy } from "~/ipc/contract";
import { agoLong, dayAndTime, startedShort } from "~/domain/format";

export const NO_ANSWER = "Windows did not say";

/** The processes running out of this install. */
export function processesFor(
  installId: string,
  running: readonly RunningComfy[],
): RunningComfy[] {
  return running.filter((p) => p.matchedInstallIds.includes(installId));
}

/** The labels of the installs a process belongs to, as they are. */
export function labelsOf(p: RunningComfy, installs: readonly Install[]): string {
  return p.matchedInstallIds
    .map((id) => installs.find((i) => i.id === id)?.label ?? id)
    .join(", ");
}

/** "8188", "8188, 8189", or null when there is no port to name. */
function portList(p: RunningComfy): string | null {
  return p.listeningPorts && p.listeningPorts.length > 0
    ? p.listeningPorts.join(", ")
    : null;
}

/**
 * The warn bar's words for the running processes, split into the part said at
 * full weight and the dimmer detail after it. A fact Windows did not give is
 * left out rather than said as unknown, because the bar is one line.
 */
export function warnbarRunning(
  running: readonly RunningComfy[],
  installs: readonly Install[],
  now: number = Date.now(),
): { lead: string; detail: string } {
  if (running.length === 1) {
    const p = running[0]!;
    const parts: string[] = [];
    if (p.startedAt) parts.push(`started ${startedShort(p.startedAt, now)}`);
    const ports = portList(p);
    if (ports) parts.push(`port ${ports}`);
    parts.push(`pid ${p.pid}`);
    return {
      lead: `${labelsOf(p, installs)} is running`,
      detail: parts.map((part) => ` · ${part}`).join(""),
    };
  }
  return {
    lead: `${running.length} ComfyUI processes are running`,
    detail: ` · ${running.map((p) => `${labelsOf(p, installs)} pid ${p.pid}`).join(", ")}`,
  };
}

/** "pid 18244, started yesterday at 18:42". */
export function pidAndStart(p: RunningComfy, now: number = Date.now()): string {
  const started = p.startedAt ? `, started ${startedShort(p.startedAt, now)}` : "";
  return `pid ${p.pid}${started}`;
}

/** "python.exe, pid 18244, started yesterday at 18:42", for the setup list. */
export function processLine(p: RunningComfy, now: number = Date.now()): string {
  return `${p.name}, ${pidAndStart(p, now)}`;
}

/**
 * Starts Task Manager, where the person ends the process themselves. When
 * Windows does not start it, the person is told the keys that do.
 */
export async function openTaskManager(app: {
  engine: Pick<Engine, "openTaskManager">;
  actions: { showToast(message: string, tone?: "ok" | "bad"): void };
}): Promise<void> {
  try {
    await app.engine.openTaskManager();
  } catch {
    app.actions.showToast(
      "Windows did not open Task Manager. Press Ctrl+Shift+Esc to open it.",
      "bad",
    );
  }
}

/** "python.exe, pid 18244, started Thu 24 Sep at 18:42", for a tooltip. */
export function processTooltip(p: RunningComfy): string {
  const started = p.startedAt ? `, started ${dayAndTime(p.startedAt)}` : "";
  return `${p.name}, pid ${p.pid}${started}`;
}

/** "Thu 24 Sep at 18:42 · 21 hours ago", or null when Windows did not say. */
export function startedFact(
  p: RunningComfy,
  now: number = Date.now(),
): { when: string; ago: string } | null {
  if (!p.startedAt) return null;
  return { when: dayAndTime(p.startedAt), ago: agoLong(p.startedAt, now) };
}

/**
 * What the port list means for the person: whether a browser can reach it.
 * Null when Windows did not say.
 */
export function listeningFact(p: RunningComfy): { value: string; note: string } | null {
  if (p.listeningPorts === null) return null;
  if (p.listeningPorts.length === 0) {
    return { value: "no port", note: "no browser tab can reach it" };
  }
  return {
    value: p.listeningPorts.map((port) => `port ${port}`).join(", "),
    note: "a browser can open it there",
  };
}

/** Null when Windows did not say. */
export function holdsFact(p: RunningComfy): string | null {
  if (p.holdsModelFiles === null) return null;
  return p.holdsModelFiles ? "holds some open" : "holds none open right now";
}

/**
 * The command line without the program itself, as a person would type it. An
 * argument with a space in it is quoted, so where one ends is not lost.
 * Null when there is nothing after the program.
 */
export function commandText(p: RunningComfy): string | null {
  const args = p.commandLine.slice(1);
  if (args.length === 0) return null;
  return args.map((a) => (/\s/.test(a) ? `"${a}"` : a)).join(" ");
}

/** Why the engine tied this process to this install. */
export function matchText(p: RunningComfy, root: string): string {
  switch (p.matchReason) {
    case "exeUnderRoot":
      return `its program file is inside ${root}`;
    case "cwdUnderRoot":
      return `it was started from inside ${root}`;
    case "argUnderRoot":
      return `its command line names a file inside ${root}`;
  }
}

/** The one port a browser can open, when there is exactly one. */
export function onlyPort(p: RunningComfy): number | null {
  return p.listeningPorts && p.listeningPorts.length === 1 ? p.listeningPorts[0]! : null;
}
