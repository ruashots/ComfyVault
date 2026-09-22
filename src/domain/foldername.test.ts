import { describe, expect, it } from "vitest";

import { folderNameError } from "~/domain/foldername";

const parent = "C:\\ComfyVault";
const siblings = ["C:\\ComfyVault\\loras", "C:\\ComfyVault\\vae"];

describe("what the picker accepts as a new folder name", () => {
  it("accepts an ordinary name", () => {
    expect(folderNameError("checkpoints", parent, siblings)).toBeNull();
    expect(folderNameError("wan 2.2", parent, siblings)).toBeNull();
    expect(folderNameError("a", parent, siblings)).toBeNull();
  });

  it("accepts a name of exactly the longest allowed length", () => {
    expect(folderNameError("x".repeat(64), parent, siblings)).toBeNull();
  });
});

describe("what the picker refuses, and why", () => {
  it("refuses an empty name", () => {
    expect(folderNameError("", parent, siblings)).toBe(
      "Type a name for the folder.",
    );
    expect(folderNameError("   ", parent, siblings)).toBe(
      "Type a name for the folder.",
    );
  });

  it.each([
    "models\\weights",
    "models/weights",
    "C:name",
    "star*",
    "what?",
    'say"hi"',
    "less<than",
    "more>than",
    "pipe|it",
  ])("refuses %s, because Windows cannot hold that character", (name) => {
    expect(folderNameError(name, parent, siblings)).toBe(
      'A folder name cannot contain \\ / : * ? " < > |',
    );
  });

  it("refuses a name padded with spaces", () => {
    expect(folderNameError(" loras2", parent, siblings)).toBe(
      "A folder name cannot start or end with a space.",
    );
    expect(folderNameError("loras2 ", parent, siblings)).toBe(
      "A folder name cannot start or end with a space.",
    );
  });

  it("refuses a name that ends with a dot", () => {
    expect(folderNameError("models.", parent, siblings)).toBe(
      "A folder name cannot end with a dot.",
    );
  });

  it("refuses a name that is too long, and says the limit", () => {
    expect(folderNameError("x".repeat(65), parent, siblings)).toBe(
      "That name is too long. Keep it under 64 characters.",
    );
  });

  it.each(["CON", "con", "nul", "LPT1", "com9", "aux.txt"])(
    "refuses %s, because Windows keeps it",
    (name) => {
      expect(folderNameError(name, parent, siblings)).toBe(
        `Windows keeps the name ${name} for itself. Choose another one.`,
      );
    },
  );

  it("refuses a name already taken here, whatever its capitalisation", () => {
    expect(folderNameError("loras", parent, siblings)).toBe(
      "There is already a folder called loras here.",
    );
    expect(folderNameError("LORAS", parent, siblings)).toBe(
      "There is already a folder called LORAS here.",
    );
  });

  it("allows a name taken somewhere else", () => {
    expect(
      folderNameError("loras", "C:\\ComfyUI-Beta\\models", siblings),
    ).toBeNull();
  });

  it("joins onto a drive root without doubling the separator", () => {
    expect(folderNameError("ComfyVault", "C:\\", ["C:\\ComfyVault"])).toBe(
      "There is already a folder called ComfyVault here.",
    );
  });
});
