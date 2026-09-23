import { readFileSync } from "node:fs";
import { join } from "node:path";

import { describe, expect, it } from "vitest";

import { FixtureEngine } from "~/ipc/fixture/engine";
import { isVaultError } from "~/ipc/contract";

/**
 * Every command but `get_app_state` refuses before a vault folder is chosen.
 *
 * The engine has a test naming each one, and this reads that list rather than
 * repeating it, so the double cannot drift from the engine on the one thing a
 * person meets before anything else. It drifted once: the double answered all
 * of them happily, the interface asked five of them the moment the window
 * opened, and a new person was told the program could not read their vault.
 */
const CONTRACT = join(
  import.meta.dirname,
  "..",
  "..",
  "docs",
  "IPC-CONTRACT.md",
);

const ENGINE_TEST = join(
  import.meta.dirname,
  "..",
  "..",
  "crates",
  "comfyvault-core",
  "src",
  "engine",
  "tests.rs",
);

function commandsThatNeedAVault(): string[] {
  const source = readFileSync(ENGINE_TEST, "utf8");
  const start = source.indexOf("fn every_command_that_needs_a_vault_says_so_before_one_is_chosen");
  expect(start, "the engine's list moved or was renamed").toBeGreaterThan(-1);
  const body = source.slice(start, source.indexOf("\n}", start));
  return [...body.matchAll(/expect\("(\w+)"/g)].map((m) => m[1]!);
}

const camel = (snake: string) =>
  snake.replace(/_([a-z0-9])/g, (_, c: string) => c.toUpperCase());

/** Arguments that get each call as far as the guard and no further. */
const ARGS: Record<string, unknown[]> = {
  buildPlan: ["scan-1"],
  registerInstall: ["C:\\ComfyUI-Alpha"],
  checkModelUsage: [["a.safetensors"]],
  getMetadata: ["A".repeat(64)],
  fetchMetadataBatch: [["A".repeat(64)]],
  updateSettings: [{}],
  getScanEntries: [{ scanId: "scan-1", offset: 0, limit: 10 }],
  listVaultFiles: [{ offset: 0, limit: 10 }],
  listContents: [{ offset: 0, limit: 10 }],
  startApply: [{ planId: "plan-1", groupIds: [] }],
  setCanonicalName: ["A".repeat(64), "a.safetensors"],
  removeAlias: ["A".repeat(64), "a.safetensors"],
  deleteVaultFile: ["A".repeat(64)],
};

/** The contract's own table of what answers before a vault is chosen. */
function commandsThatAnswer(): string[] {
  const source = readFileSync(CONTRACT, "utf8");
  const start = source.indexOf("### 1.5 Before a vault is chosen");
  expect(start, "the contract's section 1.5 moved or was renamed").toBeGreaterThan(-1);
  const section = source.slice(start, source.indexOf("\n### ", start + 10));
  const table = [...section.matchAll(/^\| `(\w+)` \|/gm)].map((m) => m[1]!);
  // `select_vault` is named in the prose under the table, as the way out.
  if (/`select_vault` is the way out/.test(section)) table.push("select_vault");
  return table;
}

/** Arguments for the handful that answer before a vault is chosen. */
const ANSWERING_ARGS: Record<string, unknown[]> = {
  validateInstallPath: ["C:\\ComfyUI-Alpha"],
  checkLockedFiles: [["C:\\ComfyUI-Alpha\\models\\a.safetensors"]],
  selectVault: ["C:\\ComfyVault"],
};

const commands = commandsThatNeedAVault();
const answering = commandsThatAnswer();

describe("before a vault folder is chosen", () => {
  it("reads the engine's own list of what refuses", () => {
    expect(commands.length).toBeGreaterThan(10);
    expect(commands).toContain("list_installs");
    expect(commands).toContain("get_vault_info");
  });

  it.each(commands)("%s refuses, the way the engine refuses", async (command) => {
    const engine = new FixtureEngine({ empty: true }) as unknown as Record<
      string,
      (...args: unknown[]) => Promise<unknown>
    >;
    const method = camel(command);
    if (typeof engine[method] !== "function") return; // not on this side yet

    let thrown: unknown = null;
    try {
      await engine[method]!(...(ARGS[method] ?? []));
    } catch (error) {
      thrown = error;
    }
    expect(thrown, `${method} answered when no vault is open`).not.toBeNull();
    expect(isVaultError(thrown) && thrown.code, `${method} refused for the wrong reason`).toBe(
      "notInitialized",
    );
  });

  it("reads the contract's list of what does answer", () => {
    expect(answering).toContain("get_app_state");
    expect(answering).toContain("validate_install_path");
    expect(answering).toContain("select_vault");
    expect(answering.length).toBeGreaterThan(3);
  });

  it.each(answering)("%s answers, because the contract says it must", async (command) => {
    const engine = new FixtureEngine({ empty: true }) as unknown as Record<
      string,
      (...args: unknown[]) => Promise<unknown>
    >;
    const method = camel(command);
    if (typeof engine[method] !== "function") return; // not on this side yet

    const answer = await engine[method]!(...(ANSWERING_ARGS[method] ?? []));
    expect(answer, `${method} gave nothing back`).not.toBeUndefined();
  });

  it("names nothing in both lists at once", () => {
    const both = answering.filter((c) => commands.includes(c));
    expect(both, "a command cannot both answer and refuse").toEqual([]);
  });

  it("answers get_app_state, because that is the one that must", async () => {
    const engine = new FixtureEngine({ empty: true });
    const state = await engine.getAppState();
    expect(state.vaultInitialized).toBe(false);
    expect(state.vaultRoot).toBeNull();
    expect(state.installCount).toBe(0);
    // Settings come back as defaults rather than refusing with everything else.
    expect(state.settings.verifyBeforeDelete).toBe(true);
    expect(state.platform).not.toBeNull();
  });
});
