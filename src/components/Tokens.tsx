import { For, Match, Show, Switch, createResource, createSignal } from "solid-js";

import { openConfirm } from "~/modals/confirm";
import { messageOf, useApp } from "~/state/store";
import type { TokenService, TokenStatus } from "~/ipc/draft";

const SERVICES: readonly { service: TokenService; name: string; placeholder: string; where: string }[] = [
  {
    service: "huggingface",
    name: "Hugging Face",
    placeholder: "Paste a Hugging Face token",
    where: "Create a read token on huggingface.co, in Settings, under Access Tokens.",
  },
  {
    service: "civitai",
    name: "Civitai",
    placeholder: "Paste a Civitai API key",
    where: "Create a key on civitai.com, in Account settings, under API Keys.",
  },
];

/**
 * The Hugging Face and Civitai tokens. The engine checks a token with its
 * service before it keeps it, and it keeps it in Windows Credential Manager.
 * The window only ever holds a token between the paste and the Save.
 */
export function TokenSection() {
  return (
    <>
      <div class="sec secgap">
        <span class="t">Hugging Face and Civitai tokens</span>
      </div>
      <div class="note" style={{ margin: "-4px 0 6px", "max-width": "720px" }}>
        Some models download only for a signed-in account. A token lets ComfyVault
        download them as you. Public models need no token. Each token is kept in
        Windows Credential Manager on this PC, not in the vault folder, so it does not
        travel with the vault.
      </div>
      <For each={SERVICES}>{(s) => <TokenRow {...s} />}</For>
    </>
  );
}

function TokenRow(props: {
  service: TokenService;
  name: string;
  placeholder: string;
  where: string;
}) {
  const app = useApp();
  const [status, { mutate, refetch }] = createResource(() =>
    app.engine.getTokenStatus(props.service),
  );
  const [typed, setTyped] = createSignal("");
  const [editing, setEditing] = createSignal(false);
  const [saving, setSaving] = createSignal(false);
  /** The service's words for a token it just refused. It was not saved. */
  const [refused, setRefused] = createSignal<string | null>(null);

  const showField = () => !status()?.saved || editing();

  const save = async () => {
    const token = typed().trim();
    if (!token || saving()) return;
    setSaving(true);
    setRefused(null);
    try {
      const answer = await app.engine.setToken(props.service, token);
      setTyped("");
      setEditing(false);
      mutate({ saved: true, ok: true, account: answer.account, message: null });
      void refetch();
    } catch (error) {
      // The typed token stays in the field, so it can be corrected.
      setRefused(messageOf(error));
    } finally {
      setSaving(false);
    }
  };

  const remove = () =>
    openConfirm(app, {
      title: "Remove a token",
      cta: "Remove it",
      body: [
        [
          {
            text: `The ${props.name} token is removed from this PC. Models that need it will not download until you add one again.`,
          },
        ],
      ],
      action: async () => {
        await app.engine.removeToken(props.service);
        setEditing(false);
        setTyped("");
        setRefused(null);
        await refetch();
      },
    });

  return (
    <div class="tok">
      <div class="tok-h">
        <span class="lb">{props.name}</span>
        <Show
          when={showField()}
          fallback={
            <>
              {/* A stand-in. The window never holds a saved token. */}
              <label class="field">
                <input
                  type="password"
                  value="••••••••••••••••••••••••"
                  readOnly
                  aria-label={`${props.name} token, saved`}
                />
              </label>
              <button class="btn sm" onClick={() => setEditing(true)}>
                Replace
              </button>
              <button class="btn sm dng" onClick={remove}>
                Remove
              </button>
            </>
          }
        >
          <label class="field">
            <input
              type="password"
              value={typed()}
              placeholder={props.placeholder}
              aria-label={`${props.name} token`}
              autocomplete="off"
              spellcheck={false}
              onInput={(e) => setTyped(e.currentTarget.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault();
                  void save();
                }
              }}
            />
          </label>
          <button
            class="btn sm"
            classList={{ pri: typed().trim().length > 0 && !saving() }}
            disabled={typed().trim().length === 0 || saving()}
            onClick={() => void save()}
          >
            {saving() ? "Checking…" : "Save"}
          </button>
          <Show when={editing()}>
            <button
              class="btn sm"
              onClick={() => {
                setEditing(false);
                setTyped("");
                setRefused(null);
              }}
            >
              Cancel
            </button>
          </Show>
        </Show>
      </div>
      <div class="tok-s" role="status">
        <Switch>
          <Match when={refused()}>
            {(message) => (
              <>
                <span class="bad">{props.name} did not accept this token.</span> It says:
                "{message()}" The token was not saved. Check it and paste it again.
              </>
            )}
          </Match>
          <Match when={status()}>{(s) => <StatusLine status={s()} {...props} />}</Match>
        </Switch>
      </div>
    </div>
  );
}

function StatusLine(props: { status: TokenStatus; service: TokenService; name: string; where: string }) {
  return (
    <Switch>
      <Match when={!props.status.saved}>
        No token. Models that need a signed-in account will not download. {props.where}
      </Match>
      <Match when={props.status.ok === true}>
        <span class="ok">Saved and working.</span>{" "}
        {props.service === "huggingface"
          ? props.status.account
            ? `Hugging Face knows it as the account ${props.status.account}. A read token is enough.`
            : "A read token is enough."
          : "Civitai accepted it."}
      </Match>
      <Match when={props.status.ok === false}>
        <span class="bad">{props.name} did not accept this token.</span>
        <Show when={props.status.message}>{(m) => <> It says: "{m()}"</>}</Show> Paste a new
        token, or remove this one.
      </Match>
      <Match when={props.status.ok === null}>
        Saved. {props.name} could not be asked about it just now, so whether it works
        is not known.
      </Match>
    </Switch>
  );
}
