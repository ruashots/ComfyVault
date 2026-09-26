import { screen } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";

import { fmt } from "~/domain/format";
import { ConfirmModalView } from "~/modals/confirm";
import { LibraryScreen } from "~/screens/Library";
import { FixtureEngine } from "~/ipc/fixture/engine";
import { renderWithApp, waitFor, type Harness } from "~/test/render";

let harness: Harness | null = null;

afterEach(() => {
  harness?.unmount();
  harness = null;
});

/** After a run, open the Library drawer on a vault file nothing links to. */
async function drawerOnUnlinkedFile() {
  const engine = new FixtureEngine({ manual: true });
  engine.devSetSymlinksSupported(true);
  engine.devSetComfyRunning(false);
  const scan = (await engine.getLastScan())!;
  const plan = await engine.buildPlan(scan.scanId);
  await engine.startApply({ planId: plan.planId, groupIds: plan.groups.map((g) => g.groupId) });
  engine.devFinish();
  harness = await renderWithApp(
    () => (
      <>
        <LibraryScreen />
        <ConfirmModalView />
      </>
    ),
    { engine },
  );
  await waitFor(() => harness!.app.orphans().length > 0);
  const orphan = harness.app.orphans()[0]!;
  await waitFor(() => harness!.app.library().some((r) => r.sha256 === orphan.sha256));
  harness.app.setLib({ selected: orphan.sha256, drawerOpen: true });
  await waitFor(() => screen.queryByRole("button", { name: /^Delete$/ }) !== null);
  return { h: harness, orphan };
}

describe("deleting a vault file from the Library drawer", () => {
  it("says the space will be freed, not that it comes back", async () => {
    const { orphan } = await drawerOnUnlinkedFile();
    const drawer = (document.body.textContent ?? "").replace(/\s+/g, " ");
    expect(drawer).toContain(`or delete it to free ${fmt(orphan.sizeBytes)}.`);
    expect(drawer).not.toContain("back.");

    await userEvent.click(screen.getByRole("button", { name: /^Delete$/ }));
    await waitFor(() => document.querySelector(".modal") !== null);
    const modal = (document.querySelector(".modal")!.textContent ?? "").replace(/\s+/g, " ");
    expect(modal).toContain(
      `will be deleted from the vault, and ${fmt(orphan.sizeBytes)} will be freed. Nothing links to it today.`,
    );
    expect(modal).not.toContain("comes back");
  });
});
