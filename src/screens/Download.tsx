import { For, Match, Show, Switch, createMemo } from "solid-js";

import { Checkbox } from "~/screens/Consolidate";
import { Icon } from "~/components/Icon";
import { UNDO_WAIT } from "~/components/UndoWait";
import { EmptyScreen, Header } from "~/components/Shell";
import {
  carriesKey,
  cutOffSentence,
  hasRoom,
  hostName,
  hostOf,
  linkHint,
  listCount,
  listOrder,
  refusedHead,
  rowView,
  type RowAction,
} from "~/domain/download";
import { fmt, joinPath, shortHash } from "~/domain/format";
import { installName } from "~/domain/installname";
import { fileNameOf, folderOf } from "~/domain/view";
import { openConfirm } from "~/modals/confirm";
import { openLinkFolderChooser } from "~/modals/linkfolder";
import { messageOf, useApp } from "~/state/store";
import type { AddressPlan, AddressRefusal, Download, DownloadHost } from "~/ipc/contract";

export function DownloadScreen() {
  const app = useApp();
  return (
    <Show
      when={app.hasVault()}
      fallback={
        <EmptyScreen
          title="Download"
          head="Nothing can be downloaded yet"
          body="A downloaded model goes into the vault, so the vault folder has to exist first."
        >
          <button class="btn pri" onClick={() => app.actions.go("home")}>
            <Icon name="arrow" size={13} />
            Finish setting up
          </button>
        </EmptyScreen>
      }
    >
      <Header
        title="Download"
        sub="From a Hugging Face or Civitai address, into the vault, linked in the installs you choose."
      />
      <div class="screen">
        <div class="scroll">
          <CutOffBanner />
          <PasteBox />
          <PlanCard />
          <Downloads />
        </div>
      </div>
    </Show>
  );
}

function CutOffBanner() {
  const app = useApp();
  const sentence = () => cutOffSentence(app.dl.downloads());
  return (
    <Show when={sentence()}>
      {(text) => (
        <div class="banner" role="status">
          <h3>
            {app.dl.downloads().filter((r) => r.state === "cutOff").length === 1
              ? "A download was cut off"
              : "Some downloads were cut off"}
          </h3>
          <p>{text()}</p>
        </div>
      )}
    </Show>
  );
}

/** The address refusals the field itself answers, below it, in red. */
function fieldRefusal(refusal: AddressRefusal | null): { head: string; next: string } | null {
  switch (refusal?.kind) {
    case "badAddress":
      return {
        head: "This is not a Hugging Face or Civitai address.",
        next: "ComfyVault downloads from huggingface.co and civitai.com only.",
      };
    case "hfRepoNotFile":
      return {
        head: "This is the address of a whole model on Hugging Face, not of one file.",
        next: 'Open the "Files and versions" tab, click the file you want, and copy that page\'s address.',
      };
    default:
      return null;
  }
}

function PasteBox() {
  const app = useApp();
  const card = app.dl.card;
  const refused = () => fieldRefusal(card.refusal);
  const read = () => {
    const address = card.address.trim();
    if (address) void app.dl.read(address);
  };

  return (
    <>
      <div class="sec">
        <span class="t">Paste the address of a model</span>
      </div>
      <div class="paste">
        <label class="field" classList={{ bad: refused() !== null }}>
          <Icon name="link" size={13} />
          <input
            value={card.address}
            placeholder="https://huggingface.co/… or https://civitai.com/models/…"
            aria-label="The address of a model on Hugging Face or Civitai"
            onInput={(e) => app.dl.setCard("address", e.currentTarget.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                read();
              }
            }}
            onPaste={(e) => {
              // Pasting is the whole gesture, so it reads at once.
              const pasted = e.clipboardData?.getData("text") ?? "";
              if (!pasted.trim()) return;
              e.preventDefault();
              app.dl.setCard("address", pasted.trim());
              read();
            }}
          />
        </label>
        <button
          class="btn"
          classList={{ pri: card.address.trim().length > 0 }}
          disabled={card.address.trim().length === 0}
          onClick={read}
        >
          Read the address
        </button>
      </div>
      <Show
        when={refused()}
        fallback={
          <div class="paste-help">
            A Hugging Face <b>file</b> address, as you get it from the file's page:
            huggingface.co/<b>owner/model</b>/blob/main/<b>file.safetensors</b>. Or a
            Civitai <b>model page</b>: civitai.com/models/<b>4384</b>, with or without
            a version.
          </div>
        }
      >
        {(r) => (
          <div class="paste-err" role="alert">
            {r().head} <span>{r().next}</span>
          </div>
        )}
      </Show>
      <Show when={carriesKey(card.address)}>
        <div class="paste-help">
          This address holds your key. ComfyVault does not keep it with the address.
          Save the key in <b>Settings</b> instead, so every model that needs it can use it.
        </div>
      </Show>
      <Show when={card.failure}>
        {(message) => (
          <div class="paste-err" role="alert">
            {message()}
          </div>
        )}
      </Show>
    </>
  );
}

function PlanCard() {
  const app = useApp();
  const card = app.dl.card;
  const plan = () => card.plan;

  return (
    <Switch>
      <Match when={card.reading}>
        <div class="card plan-card">
          <div class="verdict wait" style={{ "margin-top": "0" }}>
            <h4>Reading the address</h4>
            <p>
              Asking {hostOf(card.address) ? hostName(hostOf(card.address)!) : "the service"}{" "}
              for the file, its size and its SHA-256. Nothing is downloaded yet.
            </p>
          </div>
          <div class="plan-acts">
            <button class="btn" onClick={() => app.dl.cancel()}>
              Cancel
            </button>
          </div>
        </div>
      </Match>
      <Match when={card.refusal && !fieldRefusal(card.refusal) ? card.refusal : null}>
        {(r) => (
          <div class="card plan-card">
            <div class="card-h">
              <span class="nm" title={refusedHead(r(), card.address).title}>
                {refusedHead(r(), card.address).title}
              </span>
              <Show when={r().host}>{(host) => <span class="src">from {hostName(host())}</span>}</Show>
            </div>
            <Show when={refusedHead(r(), card.address).subtitle}>
              {(sub) => (
                <div class="card-sub" title={sub()}>
                  {sub()}
                </div>
              )}
            </Show>
            <Refused refusal={r()} />
          </div>
        )}
      </Match>
      <Match when={plan()}>
        {(p) => (
          <div class="card plan-card">
            <CardHead plan={p()} />
            <Show when={p().alreadyInVault} fallback={<Ready plan={p()} />}>
              {(already) => <Already plan={p()} vaultRelPath={already().vaultRelPath} />}
            </Show>
          </div>
        )}
      </Match>
    </Switch>
  );
}

/** The id of the file the plan is for, to read it again with another folder. */
const fileIdOf = (plan: AddressPlan) => plan.fileId ?? undefined;

function CardHead(props: { plan: AddressPlan }) {
  const app = useApp();
  const p = () => props.plan;
  const version = () => p().versions.find((v) => v.id === p().versionId);
  const reread = (choice: { versionId?: number; fileId?: number }) =>
    void app.dl.read(app.dl.card.address, {
      ...choice,
      ...(app.dl.card.category ? { category: app.dl.card.category } : {}),
    });

  return (
    <>
      <div class="card-h">
        <span class="nm" title={p().title}>
          {p().title}
          <Show when={version()}>{(v) => <>, version {v().name}</>}</Show>
        </span>
        <span class="src">from {hostName(p().host)}</span>
      </div>
      <div class="card-sub" title={p().subtitle}>
        {p().subtitle}
      </div>
      <Show when={p().host === "civitai" && p().versions.length > 0}>
        <div class="pick">
          <label>
            Version
            <select
              class="pick-sel"
              aria-label="Version"
              onChange={(e) => reread({ versionId: Number(e.currentTarget.value) })}
            >
              <For each={p().versions}>
                {(v, i) => (
                  <option value={v.id} selected={v.id === p().versionId}>
                    {v.name}
                    {i() === 0 ? " (newest)" : ""}
                  </option>
                )}
              </For>
            </select>
          </label>
          <label>
            File
            <select
              class="pick-sel"
              aria-label="File"
              onChange={(e) =>
                reread({
                  ...(p().versionId !== null ? { versionId: p().versionId! } : {}),
                  fileId: Number(e.currentTarget.value),
                })
              }
            >
              <For each={p().files}>
                {(f) => (
                  <option value={f.id} selected={f.id === p().fileId}>
                    {f.name}, {fmt(f.sizeBytes)}
                    {f.detail ? `, ${f.detail}` : ""}
                  </option>
                )}
              </For>
            </select>
          </label>
        </div>
      </Show>
    </>
  );
}

/** The path a link will have, the folder cut first. */
function LinkPath(props: { path: string; full?: string }) {
  return (
    <span class="pp" title={props.full ?? props.path}>
      <span class="pd">{folderOf(props.path)}</span>
      <span class="pf">{fileNameOf(props.path)}</span>
    </span>
  );
}

/** Every install, and whether the file will be linked in it. */
function InstallLines(props: { plan: AddressPlan }) {
  const app = useApp();
  const card = app.dl.card;
  const toggle = (id: string) =>
    app.dl.setCard("ticked", (t) => (t.includes(id) ? t.filter((x) => x !== id) : [...t, id]));

  return (
    <For each={props.plan.installs}>
      {(line) => {
        const install = () => app.installs().find((i) => i.id === line.installId);
        const name = () => (install() ? installName(install()!, app.installs()) : line.installId);
        const on = () => card.ticked.includes(line.installId);
        // The folder the person chose, else the engine's default for this install.
        const chosenDir = () => card.dirs[line.installId] ?? line.defaultDir;
        // Without a folder the path is not known yet, so the folder shows as "…".
        const path = () =>
          chosenDir()
            ? joinPath(chosenDir()!, props.plan.fileName)
            : (line.linkPath ?? `${install()?.modelsDir ?? ""}\\…\\${props.plan.fileName}`);
        // Inside the install the path starts at the install folder, which the row names.
        const shown = (p: string) =>
          install() && p.toLowerCase().startsWith(`${install()!.root.toLowerCase()}\\`)
            ? p.slice(install()!.root.length + 1)
            : p;
        return (
          <Switch>
            <Match when={line.state === "hasLink"}>
              <div class="dl-inst">
                <span class="cb on held">
                  <Icon name="check" size={9} />
                </span>
                <span class="nm" title={install()?.root}>
                  {name()}
                </span>
                <span class="why">already has this link</span>
              </div>
            </Match>
            <Match when={line.state === "nameTaken"}>
              <div class="dl-inst off">
                <span class="cb held">
                  <Icon name="check" size={9} />
                </span>
                <span class="nm" title={install()?.root}>
                  {name()}
                </span>
                <span class="why bad" title={line.linkPath ?? undefined}>
                  a different file already has this name in its {props.plan.category} folder
                </span>
              </div>
            </Match>
            <Match when={line.state === "unavailable"}>
              <div class="dl-inst off">
                <span class="cb held">
                  <Icon name="check" size={9} />
                </span>
                <span class="nm" title={install()?.root}>
                  {name()}
                </span>
                <span class="why">its folder cannot be reached right now</span>
              </div>
            </Match>
            <Match when={line.state === "free"}>
              <div class="dl-inst" classList={{ off: !on() }}>
                <Checkbox
                  on={on()}
                  label={`Link it in ${name()}`}
                  onToggle={() => toggle(line.installId)}
                />
                <span class="nm" title={install()?.root}>
                  {name()}
                </span>
                <Show
                  when={on() && props.plan.category && line.defaultDir}
                  fallback={<LinkPath path={shown(path())} full={path()} />}
                >
                  <button
                    class="pp pickp"
                    title={`${path()}. Click to choose another folder.`}
                    aria-label={`Choose the folder the link goes in, in ${name()}`}
                    onClick={() =>
                      openLinkFolderChooser(app, {
                        installId: line.installId,
                        category: props.plan.category!,
                        fileName: props.plan.fileName,
                        current: chosenDir(),
                        onUse: (dir) => app.dl.setCard("dirs", line.installId, dir),
                      })
                    }
                  >
                    <span class="pd">{folderOf(shown(path()))}</span>
                    <span class="pf">{fileNameOf(path())}</span>
                    <Icon name="folder" size={11} />
                  </button>
                </Show>
              </div>
            </Match>
          </Switch>
        );
      }}
    </For>
  );
}

/** The ticked installs that will get a link. */
const tickedFree = (plan: AddressPlan, ticked: readonly string[]) =>
  plan.installs.filter((i) => i.state === "free" && ticked.includes(i.installId));

function Ready(props: { plan: AddressPlan }) {
  const app = useApp();
  const card = app.dl.card;
  const p = () => props.plan;
  const volume = () => app.vaultVolume();
  const room = () => hasRoom(p());
  const ticked = createMemo(() => tickedFree(p(), card.ticked));
  const chosenByPerson = () => card.category !== null;

  const chooseFolder = (category: string) => {
    app.dl.setCard("category", category);
    void app.dl.read(card.address, {
      ...(p().versionId !== null ? { versionId: p().versionId! } : {}),
      ...(fileIdOf(p()) !== undefined ? { fileId: fileIdOf(p())! } : {}),
      category,
    });
  };

  return (
    <>
      <div class="plan">
        <div class="kv">
          <span class="k">Will download</span>
          <span class="v">
            <b>{p().fileName}</b>, {fmt(p().sizeBytes)}
          </span>
        </div>
        <div class="kv">
          <span class="k">Will go into the vault as</span>
          <span class="v">
            <Show
              when={p().category}
              fallback={
                <>
                  <span class="faint">the folder you choose</span>, as {p().fileName}
                </>
              }
            >
              <b>{joinPath(app.vault()?.root ?? "", p().vaultRelPath!)}</b>
              <Show when={p().vaultNameTaken}>
                <span class="faint"> (another model already has this name in the vault)</span>
              </Show>
              <Show when={!chosenByPerson() && p().suggestedBecause}>
                {(why) => <span class="faint"> ({why()})</span>}
              </Show>
            </Show>
            <div style={{ "margin-top": "5px" }}>
              <select
                class="pick-sel"
                classList={{ need: p().category === null }}
                aria-label="Folder"
                onChange={(e) => chooseFolder(e.currentTarget.value)}
              >
                <Show when={p().category === null}>
                  <option value="" selected disabled>
                    Choose a folder
                  </option>
                </Show>
                <For each={p().categories}>
                  {(c) => (
                    <option value={c} selected={c === p().category}>
                      {c}
                    </option>
                  )}
                </For>
              </select>
            </div>
          </span>
        </div>
        <div class="kv">
          <span class="k">
            Will be linked in
            <span class="khint">Click a path to choose the folder the link goes in.</span>
          </span>
          <span class="v">
            <InstallLines plan={p()} />
            <Show when={ticked().length === 0}>
              <div class="note" style={{ "margin-top": "4px" }}>
                No install is ticked. The file will only be in the vault, and you can
                link it later from Library.
              </div>
            </Show>
          </span>
        </div>
        <div class="kv">
          <span class="k">Will leave free on drive {volume()}</span>
          <span class="v">
            <Switch>
              <Match when={p().vaultFreeBytes === null}>
                <span class="red">
                  ComfyVault could not read how much free space drive {volume()} has.
                </span>{" "}
                <span class="dim">Check that the drive is connected, then read the address again.</span>
              </Match>
              <Match when={room()}>
                <b>{fmt(p().vaultFreeBytes! - p().sizeBytes)}</b>{" "}
                <span class="dim">of the {fmt(p().vaultFreeBytes!)} free now</span>
              </Match>
              <Match when={!room()}>
                <span class="red">
                  Drive {volume()} has {fmt(p().vaultFreeBytes!)} free, and this file needs{" "}
                  {fmt(p().sizeBytes)}.
                </span>{" "}
                <span class="dim">Free some space, then read the address again.</span>
              </Match>
            </Switch>
          </span>
        </div>
        <div class="kv">
          <span class="k">Will be checked against</span>
          <span class="v">
            <Show
              when={p().sha256}
              fallback={
                <span class="dim">
                  its own SHA-256, worked out after the download.{" "}
                  <span class="faint">
                    {hostName(p().host)} gives none for this file, so ComfyVault checks
                    the vault for it once it is here.
                  </span>
                </span>
              }
            >
              {(sha) => (
                <>
                  SHA-256 <span class="dim">{shortHash(sha())}</span>{" "}
                  <span class="faint">
                    from {hostName(p().host)}. A file that does not match is deleted.
                  </span>
                </>
              )}
            </Show>
          </span>
        </div>
      </div>
      <div class="plan-acts">
        <Switch>
          <Match when={p().category === null}>
            <button class="btn" disabled>
              Choose a folder first
            </button>
          </Match>
          <Match when={p().vaultFreeBytes === null}>
            <button class="btn" disabled>
              Free space on drive {volume()} is not known
            </button>
          </Match>
          <Match when={!room()}>
            <button class="btn" disabled>
              Not enough space on drive {volume()}
            </button>
          </Match>
          <Match when={true}>
            <button
              class="btn pri"
              disabled={card.starting}
              onClick={() => void start(app, p(), ticked().map((t) => t.installId))}
            >
              <Icon name="download" size={13} />
              Download {fmt(p().sizeBytes)}
            </button>
          </Match>
        </Switch>
        <button class="btn" onClick={() => app.dl.cancel()}>
          Cancel
        </button>
        <Show when={p().category !== null && room() ? linkHint(ticked().length) : null}>
          {(hint) => <span class="hint">{hint()}</span>}
        </Show>
      </div>
    </>
  );
}

/** Queue the download the card describes. The engine keeps the ticks for next time. */
async function start(app: ReturnType<typeof useApp>, plan: AddressPlan, installIds: string[]) {
  const card = app.dl.card;
  if (card.starting || plan.category === null) return;
  app.dl.setCard("starting", true);
  try {
    await app.engine.startDownload({
      address: card.address,
      ...(plan.versionId !== null ? { versionId: plan.versionId } : {}),
      ...(fileIdOf(plan) !== undefined ? { fileId: fileIdOf(plan)! } : {}),
      category: plan.category,
      installIds,
      // Each link in the folder chosen for it, or the engine's default.
      links: installIds.map((id) => ({
        installId: id,
        dir:
          card.dirs[id] ??
          plan.installs.find((i) => i.installId === id)?.defaultDir ??
          "",
      })).filter((l) => l.dir !== ""),
    });
    app.dl.cancel();
    app.dl.setCard("address", "");
    await app.dl.load();
  } catch (error) {
    app.dl.setCard("starting", false);
    app.actions.showToast(messageOf(error), "bad");
  }
}

function Already(props: { plan: AddressPlan; vaultRelPath: string }) {
  const app = useApp();
  const card = app.dl.card;
  const p = () => props.plan;
  const ticked = createMemo(() => tickedFree(p(), card.ticked));
  const free = () => p().installs.filter((i) => i.state === "free").length;

  return (
    <>
      <div class="verdict ok">
        <h4>
          <Icon name="check" size={12} />
          Already in the vault
        </h4>
        <p>
          The vault already has this file as{" "}
          <span class="emph">{joinPath(app.vault()?.root ?? "", props.vaultRelPath)}</span>,
          with the same SHA-256. <span class="emph">Nothing will be downloaded.</span>
        </p>
      </div>
      <div class="plan">
        <div class="kv">
          <span class="k">
            Will be linked in
            <span class="khint">Click a path to choose the folder the link goes in.</span>
          </span>
          <span class="v">
            <InstallLines plan={p()} />
          </span>
        </div>
      </div>
      <div class="plan-acts">
        <Switch>
          <Match when={free() === 0}>
            <button class="btn" disabled>
              Every install already has this link
            </button>
          </Match>
          <Match when={ticked().length === 0}>
            <button class="btn" disabled>
              Tick an install to link it
            </button>
          </Match>
          {/* The links are made at once, and links wait for an undo in the engine. */}
          <Match when={app.undoRunning()}>
            <button class="btn" disabled>
              {UNDO_WAIT}
            </button>
          </Match>
          <Match when={true}>
            <button
              class="btn pri"
              disabled={card.starting}
              onClick={() => void start(app, p(), ticked().map((t) => t.installId))}
            >
              Add {ticked().length === 1 ? "1 link" : `${ticked().length} links`}
            </button>
          </Match>
        </Switch>
        <button class="btn" onClick={() => app.dl.cancel()}>
          Cancel
        </button>
      </div>
    </>
  );
}

interface RefusalWords {
  head: string;
  cause: string;
  next: string;
  /** The first button: Settings, or the model's page. */
  first: "settings" | "page" | null;
}

/** What a refusal found while reading says, by service and kind. */
export function refusalWords(
  host: DownloadHost,
  kind: AddressRefusal["kind"],
): RefusalWords {
  const hf = host === "huggingface";
  switch (kind) {
    case "tokenMissing":
      return hf
        ? {
            head: "Hugging Face needs your token for this model",
            cause:
              "This model is gated: Hugging Face only lets a signed-in account with access download it. ComfyVault has no Hugging Face token yet.",
            next: "Add your Hugging Face token in Settings, then read the address again.",
            first: "settings",
          }
        : {
            head: "Civitai needs your token for this model",
            cause:
              "The creator only lets signed-in accounts download it. ComfyVault has no Civitai token yet.",
            next: "Add your Civitai token in Settings, then read the address again.",
            first: "settings",
          };
    case "tokenRejected":
      return hf
        ? {
            head: "Hugging Face did not accept your token",
            cause:
              "The token saved in Settings is not valid any more. It was probably deleted or replaced on huggingface.co.",
            next: "Paste a new token in Settings, then read the address again.",
            first: "settings",
          }
        : {
            head: "Civitai did not accept your token",
            cause:
              "The token saved in Settings is not valid any more. It was probably deleted or replaced on civitai.com.",
            next: "Paste a new token in Settings, then read the address again.",
            first: "settings",
          };
    case "noAccess":
      return hf
        ? {
            head: "Your Hugging Face account has no access to this model yet",
            cause:
              "Your token works, but the model's owner asks each account to accept its terms first.",
            next: "Open the model's page, accept the terms with the same account, then read the address again.",
            first: "page",
          }
        : {
            head: "Your Civitai account has no access to this model",
            cause: "Your token works, but Civitai does not let this account download the model.",
            next: "Open the model's page to see why, then read the address again.",
            first: "page",
          };
    case "notFound":
    case "badAddress":
    case "hfRepoNotFile":
      return {
        head: `${hostName(host)} has no file at this address`,
        cause: "The address reads correctly, but the file it names is not there.",
        next: "Check the address on the model's page, then read it again.",
        first: null,
      };
  }
}

function Refused(props: { refusal: AddressRefusal }) {
  const app = useApp();
  const host = () => props.refusal.host ?? "huggingface";
  const words = () => {
    const w = refusalWords(host(), props.refusal.kind);
    // Only a Hugging Face refusal names a page ComfyVault can open.
    return w.first === "page" && !props.refusal.page ? { ...w, first: null } : w;
  };
  const again = () => void app.dl.read(app.dl.card.address);

  return (
    <>
      <div class="verdict no" role="alert">
        <h4>
          <Icon name="warn" size={12} />
          {words().head}
        </h4>
        <p>
          {words().cause}
          <Show when={props.refusal.serviceMessage}> {hostName(host())} says:</Show>
        </p>
        <Show when={props.refusal.serviceMessage}>
          {(said) => <q>{said()}</q>}
        </Show>
        <p style={{ "margin-top": "8px" }}>{words().next}</p>
      </div>
      <div class="plan-acts">
        <Switch>
          <Match when={words().first === "settings"}>
            <button class="btn pri" onClick={() => app.actions.go("settings")}>
              Open Settings
            </button>
          </Match>
          <Match when={words().first === "page" ? props.refusal.page : null}>
            {(page) => (
              <button
                class="btn pri"
                onClick={() =>
                  void app.engine
                    .openHuggingFacePage(page().owner, page().repo)
                    .catch((error: unknown) => app.actions.showToast(messageOf(error), "bad"))
                }
              >
                <Icon name="external" size={13} />
                Open the model's page
              </button>
            )}
          </Match>
        </Switch>
        <button class="btn" classList={{ pri: words().first === null }} onClick={again}>
          Read the address again
        </button>
      </div>
    </>
  );
}

function Downloads() {
  const app = useApp();
  const records = () => app.dl.downloads();
  return (
    <>
      <div class="sec secgap">
        <span class="t">Downloads</span>
        <Show when={records().length > 0}>
          <span class="n">{listCount(records())}</span>
        </Show>
      </div>
      <Show
        when={records().length > 0}
        fallback={
          <div class="note">
            Nothing has been downloaded yet. A download you start shows here, and it
            stays here until ComfyVault closes.
          </div>
        }
      >
        <div class="note" style={{ margin: "-4px 0 4px" }}>
          One download runs at a time. The others wait their turn.
        </div>
        <For each={listOrder(records())}>{(r) => <DownloadRow record={r} />}</For>
      </Show>
    </>
  );
}

const ACTION_WORDS: Record<RowAction, string> = {
  stop: "Stop",
  remove: "Remove from the list",
  continue: "Continue",
  discard: "Discard it",
  again: "Download it again",
  library: "Show it in Library",
};

function DownloadRow(props: { record: Download }) {
  const app = useApp();
  const r = () => props.record;
  const view = createMemo(() => rowView(r(), app.installs(), app.vault()?.root ?? ""));

  /** The model is found in the vault by its SHA-256, or else by where it went. */
  const showInLibrary = async () => {
    const sha = r().sha256;
    const where = r().vaultRelPath.toLowerCase();
    const find = () =>
      app
        .vaultFiles()
        .find((f) => (sha ? f.sha256 === sha : f.vaultRelPath.toLowerCase() === where));
    // The list of vault files may not have caught up with a download that just ended.
    if (!find()) await app.actions.refresh();
    const file = find();
    app.setLib(
      file
        ? { selected: file.sha256, drawerOpen: true, query: "", category: "all", unusedOnly: false }
        : { selected: null, drawerOpen: false, query: r().fileName, category: "all", unusedOnly: false },
    );
    app.actions.go("library");
  };

  const act = (action: RowAction) => {
    const id = r().downloadId;
    const take = (call: () => Promise<Download>) =>
      void call()
        .then((record) => app.dl.receive(record))
        .catch((error: unknown) => app.actions.showToast(messageOf(error), "bad"));
    switch (action) {
      case "stop":
        return take(() => app.engine.stopDownload(id));
      case "continue":
      case "again":
        return take(() => app.engine.continueDownload(id));
      case "remove":
        return void app.engine
          .removeDownload(id)
          .then(() => app.dl.forget(id))
          .catch((error: unknown) => app.actions.showToast(messageOf(error), "bad"));
      case "discard":
        return openConfirm(app, {
          title: "Discard a download",
          cta: "Discard it",
          body: [
            [
              { text: `The ${fmt(r().bytesDone)} already downloaded is deleted.` },
              { text: " The model is not in the vault and not linked anywhere." },
            ],
          ],
          action: async () => {
            await app.engine.discardDownload(id);
            app.dl.forget(id);
          },
        });
      case "library":
        return void showInLibrary();
    }
  };

  return (
    <div class="dlrow" classList={{ wait: r().state === "waiting" }}>
      <div class="dlrow-h">
        <span class="nm" title={r().fileName}>
          {r().fileName}
        </span>
        <span class="sz">{fmt(r().bytesTotal)}</span>
      </div>
      <div class="dlrow-s">
        <For each={view().parts}>
          {(part) => (
            <Show when={part.tone} fallback={<>{part.text}</>}>
              <span class={part.tone}>{part.text}</span>
            </Show>
          )}
        </For>
      </div>
      <Show when={view().bar}>
        {(bar) => (
          <div class="bar" classList={{ stop: bar().stopped }}>
            <i style={{ width: `${Math.round(bar().fraction * 100)}%` }} />
          </div>
        )}
      </Show>
      <Show when={view().actions.length > 0}>
        <div class="dlrow-a">
          <For each={view().actions}>
            {(action, i) => (
              <button
                class="btn sm"
                classList={{ pri: i() === 0 && ["continue", "again"].includes(action) }}
                onClick={() => act(action)}
              >
                {ACTION_WORDS[action]}
              </button>
            )}
          </For>
        </div>
      </Show>
    </div>
  );
}
