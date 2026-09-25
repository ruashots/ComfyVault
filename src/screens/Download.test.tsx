import { afterEach, describe, expect, it } from "vitest";

import { FixtureEngine } from "~/ipc/fixture/engine";
import { DownloadScreen } from "~/screens/Download";
import { renderWithApp, type Harness } from "~/test/render";

let harness: Harness | null = null;

afterEach(() => {
  harness?.unmount();
  harness = null;
});

describe("where a download would go", () => {
  it("names no folder before a vault is chosen, since none is offered", async () => {
    harness = await renderWithApp(() => <DownloadScreen />, {
      engine: new FixtureEngine({ empty: true }),
    });
    const text = document.body.textContent ?? "";
    expect(text).not.toContain("C:\\ComfyVault");
    expect(text).toContain("the vault folder, once one is chosen");
  });

  it("names the vault once there is one", async () => {
    harness = await renderWithApp(() => <DownloadScreen />, { engine: new FixtureEngine() });
    expect(document.body.textContent).toContain(harness.app.vault()!.root);
  });
});
