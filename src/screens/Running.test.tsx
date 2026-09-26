import { screen } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";

import { App } from "~/App";
import { FixtureEngine, type ProcessFacts } from "~/ipc/fixture/engine";
import { renderWithApp, waitFor, type Harness } from "~/test/render";

let harness: Harness | null = null;

afterEach(() => {
  harness?.unmount();
  harness = null;
});

/** Local time, the day before today, at this hour. */
function yesterday(hour: number, minute: number): string {
  const d = new Date();
  return new Date(d.getFullYear(), d.getMonth(), d.getDate() - 1, hour, minute).toISOString();
}

/**
 * The scanned world, with ComfyUI running out of ComfyUI-Studio and Windows
 * answering as `facts` says.
 */
async function running(
  facts: Partial<ProcessFacts> = {},
  prepare?: (engine: FixtureEngine) => void,
): Promise<Harness> {
  const engine = new FixtureEngine();
  engine.devSetSymlinksSupported(true);
  engine.devSetProcessFacts({ startedAt: yesterday(18, 42), ...facts });
  prepare?.(engine);
  harness = await renderWithApp(() => <App />, { engine });
  await waitFor(() => harness!.app.running().length > 0);
  return harness;
}

const bar = () => document.querySelector(".warnbar .wt");
const button = (name: string | RegExp) => screen.getByRole("button", { name });
const fact = (label: string) =>
  [...document.querySelectorAll(".proc .pr")]
    .find((row) => row.querySelector(".pk")!.textContent === label)
    ?.querySelector(".pv")!.textContent ?? null;
const toast = () => document.querySelector(".toast")?.textContent ?? "";

async function consolidate(h: Harness): Promise<void> {
  h.app.actions.go("consolidate");
  await waitFor(() => document.querySelector(".proc") !== null);
}

describe("the warn bar names what is running", () => {
  it("gives the install, when it started, its port and its pid", async () => {
    await running();
    expect(bar()!.textContent).toBe(
      "ComfyUI-Studio is running · started yesterday at 18:42 · port 8188 · pid 18244",
    );
    // The name is said at full weight, the facts after it dimmer.
    expect(bar()!.querySelector("b")!.textContent).toBe("ComfyUI-Studio is running");
    expect(bar()!.querySelector("i")!.textContent).toBe(
      " · started yesterday at 18:42 · port 8188 · pid 18244",
    );
    expect(button("See what to fix")).toBeInTheDocument();
  });

  it("leaves out a port when there is none, and a start Windows did not give", async () => {
    await running({ listeningPorts: [], startedAt: null });
    expect(bar()!.textContent).toBe("ComfyUI-Studio is running · pid 18244");
  });

  it("leaves out the port when Windows did not say", async () => {
    await running({ listeningPorts: null });
    expect(bar()!.textContent).toBe(
      "ComfyUI-Studio is running · started yesterday at 18:42 · pid 18244",
    );
  });

  it("names each process when there is more than one", async () => {
    await running({}, (engine) => engine.devSetRunningInstalls(["studio", "sandbox"]));
    expect(bar()!.textContent).toBe(
      "2 ComfyUI processes are running · ComfyUI-Studio pid 18244, ComfyUI-Sandbox pid 18245",
    );
  });

  it("names both things when a second one blocks Apply too", async () => {
    await running({}, (engine) => engine.devSetSymlinksSupported(false));
    expect(bar()!.textContent).toBe(
      "Windows will not let this app create links · ComfyUI-Studio is running · started yesterday at 18:42 · port 8188 · pid 18244",
    );
    expect(bar()!.textContent).not.toContain("things block Apply");
  });

  it("carries the whole sentence for a window too narrow to show it", async () => {
    await running();
    expect(bar()!.getAttribute("title")).toBe(bar()!.textContent);
  });
});

describe("the Apply blocker for a running ComfyUI", () => {
  it("titles it with the label as it is, never doubled", async () => {
    const h = await running();
    await consolidate(h);
    const titles = [...document.querySelectorAll(".blkrow .bt")].map((e) => e.textContent);
    expect(titles).toContain("ComfyUI-Studio is running");
    expect(document.body.textContent).not.toContain("ComfyUI-ComfyUI-");
  });

  it("lists every fact Windows gave, each with its label", async () => {
    const h = await running({
      commandLine: [
        "C:\\ComfyUI-Studio\\python_embeded\\python.exe",
        "-s",
        "C:\\ComfyUI Studio\\main.py",
        "--port",
        "8188",
      ],
    });
    await consolidate(h);
    expect(fact("Process")).toBe("python.exe · pid 18244");
    expect(fact("Started")).toMatch(/^\w{3} \d{1,2} \w{3} at 18:42 · \d+ hours ago$/);
    expect(fact("Listening on")).toBe("port 8188 · a browser can open it there");
    expect(fact("Model files")).toBe("holds some open");
    expect(fact("Program")).toBe("C:\\ComfyUI-Studio\\python_embeded\\python.exe");
    // The program itself is left off, and an argument with a space is quoted.
    expect(fact("Command")).toBe('-s "C:\\ComfyUI Studio\\main.py" --port 8188');
    expect(fact("Why this install")).toBe("its program file is inside C:\\ComfyUI-Studio");
    expect(document.querySelector(".blkrow .how")!.textContent).toBe(
      "Closing the browser tab leaves ComfyUI running. Stop it where you started it, or open Task Manager, go to the Details tab and end pid 18244. ComfyVault does not stop programs itself.",
    );
  });

  it("says Windows did not say, and guesses nothing, where there was no answer", async () => {
    const h = await running({ startedAt: null, listeningPorts: null, holdsModelFiles: null });
    await consolidate(h);
    expect(fact("Started")).toBe("Windows did not say");
    expect(fact("Listening on")).toBe("Windows did not say");
    expect(fact("Model files")).toBe("Windows did not say");
    expect(screen.queryByRole("button", { name: /Open 127\.0\.0\.1/ })).toBeNull();
  });

  it("says a process on no port serves no browser tab, and offers none", async () => {
    const h = await running({ listeningPorts: [], holdsModelFiles: false });
    await consolidate(h);
    expect(fact("Listening on")).toBe("no port · no browser tab can reach it");
    expect(fact("Model files")).toBe("holds none open right now");
    expect(screen.queryByRole("button", { name: /Open 127\.0\.0\.1/ })).toBeNull();
  });

  it("leaves out the command when there is nothing after the program", async () => {
    const h = await running({ commandLine: ["python.exe"] });
    await consolidate(h);
    expect(fact("Command")).toBeNull();
  });

  it("still blocks Apply for a ComfyUI that holds no model file right now", async () => {
    const h = await running({ listeningPorts: [], holdsModelFiles: false });
    await consolidate(h);
    expect(h.app.gate().can).toBe(false);
    const apply = screen
      .getAllByRole("button")
      .find((b) => /apply/i.test(b.textContent ?? ""))!;
    expect(apply).toBeDisabled();
  });

  it("opens the one port it listens on in the browser", async () => {
    const h = await running();
    await consolidate(h);
    await userEvent.click(button("Open 127.0.0.1:8188"));
    expect((h.engine as FixtureEngine).opened).toEqual(["http://127.0.0.1:8188/"]);
  });

  it("offers no page when it listens on more than one port", async () => {
    const h = await running({ listeningPorts: [8188, 8189] });
    await consolidate(h);
    expect(fact("Listening on")).toBe("port 8188, port 8189 · a browser can open it there");
    expect(screen.queryByRole("button", { name: /Open 127\.0\.0\.1/ })).toBeNull();
  });

  it("opens Task Manager, and ends nothing itself", async () => {
    const h = await running();
    await consolidate(h);
    await userEvent.click(button("Open Task Manager"));
    await waitFor(() => (h.engine as FixtureEngine).taskManagerOpened === 1);
    expect(h.app.running()).toHaveLength(1);
  });

  it("gives the keys to press when Windows does not start Task Manager", async () => {
    const h = await running({}, (engine) => engine.devSetTaskManagerStarts(false));
    await consolidate(h);
    await userEvent.click(button("Open Task Manager"));
    await waitFor(() => toast() !== "");
    expect(toast()).toBe(
      "Windows did not open Task Manager. Press Ctrl+Shift+Esc to open it.",
    );
  });

  it("names the process when a check finds it still running", async () => {
    const h = await running();
    await consolidate(h);
    await userEvent.click(button("Check again"));
    await waitFor(() => toast() !== "");
    expect(toast()).toBe("Checked · ComfyUI-Studio is still running, pid 18244");
  });

  it("says so when a check finds it gone", async () => {
    const h = await running();
    await consolidate(h);
    (h.engine as FixtureEngine).devSetComfyRunning(false);
    await userEvent.click(button("Check again"));
    await waitFor(() => toast() !== "");
    expect(toast()).toMatch(/^Checked · nothing is in the way, .+ can be freed$/);
  });
});

describe("the other places a running ComfyUI shows", () => {
  it("Settings names it, when it started, and offers Task Manager", async () => {
    const h = await running();
    h.app.actions.go("settings");
    await waitFor(() => document.body.textContent?.includes("ComfyUI processes") === true);
    const row = [...document.querySelectorAll(".chk")].find((r) =>
      r.textContent?.includes("ComfyUI processes"),
    )!;
    expect(row.querySelector(".sp")!.textContent).toBe(
      "ComfyUI-Studio is running · pid 18244, started yesterday at 18:42 · its open files cannot move",
    );
    const names = [...row.querySelectorAll("button")].map((b) => b.textContent);
    expect(names).toEqual(["Open Task Manager", "Check again"]);
  });

  it("the Home pill says which process it is", async () => {
    await running();
    const pill = document.querySelector(".inst .pill.up")!;
    expect(pill.textContent).toBe("running");
    expect(pill.getAttribute("title")).toMatch(
      /^python\.exe, pid 18244, started \w{3} \d{1,2} \w{3} at 18:42$/,
    );
  });
});
