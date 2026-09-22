import { For, Show, createMemo } from "solid-js";

import { Icon, Mark } from "~/components/Icon";
import { Header, Warnbar } from "~/components/Shell";
import { fmt, fmtN, fmtU, relativeTime, usedPercent } from "~/domain/format";
import { openInstancePicker } from "~/modals/picker";
import { useApp } from "~/state/store";
import { ScanScreen } from "~/screens/Scan";

export function HomeScreen() {
  const app = useApp();
  return (
    <Show when={app.hasInstances()} fallback={<FirstRun />}>
      <Show when={!app.scanProgress()} fallback={<ScanScreen />}>
        <HomeReport />
      </Show>
    </Show>
  );
}

function FirstRun() {
  const app = useApp();
  return (
    <>
      <Header title="Home" sub="no instances yet" />
      <div class="screen">
        <div class="empty">
          <Mark size={34} />
          <h2>Nothing registered yet</h2>
          <p>
            ComfyVault needs to know where your ComfyUI installs are. It reads each
            one, then holds one copy of every model in a single folder and leaves a
            link behind in every place a file used to be.
          </p>
          <div class="acts">
            <button class="btn pri" onClick={() => void openInstancePicker(app)}>
              <Icon name="folder" size={13} />
              Choose an install folder
            </button>
          </div>
          <div class="foot">
            The vault will be created at{" "}
            <span class="emph">{app.machine()?.vaultPath}</span>.
            <br />
            Change that in Settings before the first scan.
          </div>
        </div>
      </div>
    </>
  );
}

function HomeReport() {
  const app = useApp();
  const plan = () => app.plan()!;
  const scan = () => app.scan()!;
  const drive = () => app.machine()!.vaultDrive;
  const run = () => app.lastRun();

  const afterFree = createMemo(
    () => drive().freeBytes + (run() ? 0 : plan().totals.reclaimBytes),
  );

  return (
    <>
      <Header title="Home" sub={`last scan ${relativeTime(scan().scannedAt)}`}>
        <button class="btn pri" onClick={() => void app.engine.startScan()}>
          <Icon name="scan" size={13} />
          Scan now
        </button>
      </Header>
      <div class="screen">
        <Warnbar />
        <div class="scroll">
          <div class="tiles">
            <Tile
              value={String(scan().instances.length)}
              label="Instances"
              note={scan().instances.map((i) => i.name).join(" · ")}
            />
            <Tile
              value={String(plan().models.length)}
              label="Unique models"
              note={`${plan().totals.files} files on disk`}
            />
            <Tile
              value={fmtN(plan().totals.onDiskBytes)}
              unit={fmtU(plan().totals.onDiskBytes)}
              label="Models on disk"
              note={`${fmt(plan().totals.uniqueBytes)} if kept once`}
            />
            <Tile
              value={String(plan().totals.unused)}
              label="Not used"
              note="name not found in any workflow"
            />
          </div>

          <Show when={run()} fallback={<PlanHero afterFree={afterFree()} />}>
            {(finished) => (
              <div class="hero done">
                <div class="big">
                  {fmtN(finished().bytesFreed)}
                  <small>{fmtU(finished().bytesFreed)}</small>
                </div>
                <div class="txt">
                  <div class="l1">
                    Freed. {finished().linksCreated} copies are now links.
                  </div>
                  <div class="l2">
                    Drive {drive().letter} has {fmt(drive().freeBytes)} free.
                  </div>
                </div>
                <button class="btn" onClick={() => app.actions.go("consolidate")}>
                  Open the report
                </button>
              </div>
            )}
          </Show>

          <div class="sec secgap">
            <span class="t">Instances</span>
            <span class="n">
              {fmt(plan().totals.onDiskBytes)} of models across{" "}
              {scan().instances.length} installs
            </span>
          </div>
          <For each={scan().instances}>
            {(instance) => {
              const totals = () => plan().totals.perInstance.get(instance.id);
              return (
                <div class="inst">
                  <span
                    class="led"
                    classList={{ up: instance.running, idle: !instance.running }}
                  />
                  <span class="name nm">{instance.name}</span>
                  <span class="path pp">{instance.path}</span>
                  <Show when={instance.extraModelPaths?.length}>
                    <span class="faint yaml">+ extra_model_paths.yaml</span>
                  </Show>
                  <span class="num c1">{totals()?.files ?? 0}</span>
                  <span class="num c2">{fmt(totals()?.bytes ?? 0)}</span>
                  <Show
                    when={instance.running}
                    fallback={<span class="pill link">idle</span>}
                  >
                    <span class="pill up">running</span>
                  </Show>
                </div>
              );
            }}
          </For>

          <div class="sec secgap">
            <span class="t">Recent</span>
          </div>
          <For each={scan().activity}>
            {(entry) => (
              <div class="act">
                <span class="a">{entry.event}</span>
                <span class="d">{entry.detail}</span>
                <span class="w">{relativeTime(entry.when)}</span>
              </div>
            )}
          </For>

          <div class="sec secgap">
            <span class="t">Counted, never moved</span>
            <span class="n">{fmt(plan().totals.countedNeverMovedBytes)}</span>
          </div>
          <For each={scan().countedNeverMoved}>
            {(entry) => (
              <div class="kv">
                <span class="k w210">
                  {entry.kind === "custom_nodes"
                    ? "Inside custom_nodes"
                    : "Hugging Face cache"}
                </span>
                <span class="v">
                  <b>{fmt(entry.bytes)}</b>{" "}
                  <span class="dim">&middot; {entry.where}</span>
                </span>
              </div>
            )}
          </For>
          <div class="note up">
            A node can load a weight straight out of either place. ComfyVault
            reports what is there and leaves both alone.
          </div>
        </div>
      </div>
    </>
  );
}

function PlanHero(props: { afterFree: number }) {
  const app = useApp();
  const plan = () => app.plan()!;
  const drive = () => app.machine()!.vaultDrive;
  return (
    <div class="hero">
      <div class="big">
        {fmtN(plan().totals.reclaimBytes)}
        <small>{fmtU(plan().totals.reclaimBytes)}</small>
      </div>
      <div class="txt">
        <Show
          when={plan().totals.duplicateCopies > 0}
          fallback={
            <>
              <div class="l1">Every model is held once already.</div>
              <div class="l2">
                Consolidating moves them into the vault and leaves a link behind,
                so nothing on drive {drive().letter} changes size.
              </div>
            </>
          }
        >
          <div class="l1">
            {plan().totals.duplicateCopies} copies of {plan().duplicates.length}{" "}
            models are held twice or more.
          </div>
          <div class="l2">
            Consolidating leaves {fmt(props.afterFree)} free on drive{" "}
            {drive().letter},{" "}
            {usedPercent(drive().totalBytes, props.afterFree)}% used.
          </div>
        </Show>
      </div>
      <button class="btn pri" onClick={() => app.actions.go("consolidate")}>
        Review the plan
        <Icon name="arrow" size={13} />
      </button>
    </div>
  );
}

function Tile(props: {
  value: string;
  unit?: string;
  label: string;
  note: string;
}) {
  return (
    <div class="tile">
      <div class="v">
        {props.value}
        <Show when={props.unit}>
          <small>{props.unit}</small>
        </Show>
      </div>
      <div class="k">{props.label}</div>
      <div class="x">{props.note}</div>
    </div>
  );
}
