import { describe, expect, it } from "vitest";

import { installName, installNameOf } from "~/domain/installname";

interface Named {
  id: string;
  label: string;
  root: string;
}

const easy: Named = {
  id: "a",
  label: "ComfyUI",
  root: "C:\\AI\\ComfyUI-Easy-Install\\ComfyUI",
};
const portable: Named = {
  id: "b",
  label: "ComfyUI",
  root: "C:\\ComfyUI_windows_portable\\ComfyUI",
};
const flux: Named = {
  id: "c",
  label: "ComfyUI",
  root: "C:\\AI\\ComfyUI-Flux\\ComfyUI",
};

describe("the name an install is shown under", () => {
  it("is its label when no other install has that label", () => {
    const studio = { id: "s", label: "ComfyUI-Studio", root: "C:\\ComfyUI-Studio" };
    expect(installName(studio, [studio, easy])).toBe("ComfyUI-Studio");
  });

  it("is the folder above the root when every root is called ComfyUI", () => {
    const all = [easy, portable, flux];
    expect(installName(easy, all)).toBe("ComfyUI-Easy-Install");
    expect(installName(portable, all)).toBe("ComfyUI_windows_portable");
    expect(installName(flux, all)).toBe("ComfyUI-Flux");
  });

  it("walks up past a folder the other install also has at that level", () => {
    const a = { id: "a", label: "ComfyUI", root: "D:\\Tools\\Stable\\app\\ComfyUI" };
    const b = { id: "b", label: "ComfyUI", root: "E:\\Tests\\Stable\\app\\ComfyUI" };
    expect(installName(a, [a, b])).toBe("Tools");
    expect(installName(b, [a, b])).toBe("Tests");
  });

  it("compares folder names the way Windows does, ignoring case", () => {
    const a = { id: "a", label: "ComfyUI", root: "C:\\One\\Portable\\ComfyUI" };
    const b = { id: "b", label: "ComfyUI", root: "D:\\Two\\portable\\ComfyUI" };
    expect(installName(a, [a, b])).toBe("One");
  });

  it("never takes a folder name that is already another install's name", () => {
    const named = { id: "n", label: "ComfyUI-Flux", root: "D:\\ComfyUI-Flux" };
    const work = { id: "w", label: "ComfyUI", root: "C:\\Work\\ComfyUI-Flux\\ComfyUI" };
    const all = [easy, work, named];
    // "ComfyUI-Flux" is unique at its level, and it is also what the other
    // install is called, so two rows would read the same.
    expect(installName(work, all)).toBe("Work");
    expect(installName(easy, all)).toBe("ComfyUI-Easy-Install");
  });

  it("is the whole root when no folder tells the two apart", () => {
    const a = { id: "a", label: "ComfyUI", root: "C:\\ComfyUI" };
    const b = { id: "b", label: "ComfyUI", root: "D:\\ComfyUI" };
    expect(installName(a, [a, b])).toBe("C:\\ComfyUI");
    expect(installName(b, [a, b])).toBe("D:\\ComfyUI");
  });

  it("reads a root written with forward slashes or a trailing separator", () => {
    const a = { id: "a", label: "ComfyUI", root: "C:/AI/Easy/ComfyUI/" };
    const b = { id: "b", label: "ComfyUI", root: "C:\\AI\\Flux\\ComfyUI" };
    expect(installName(a, [a, b])).toBe("Easy");
  });
});

describe("the name of an install known only by its id", () => {
  it("looks the install up, so a label copied into a row is never shown", () => {
    expect(installNameOf("b", [easy, portable])).toBe("ComfyUI_windows_portable");
  });

  it("is the id when the install is no longer registered", () => {
    expect(installNameOf("gone", [easy, portable])).toBe("gone");
  });
});
