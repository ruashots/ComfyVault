import { For, Match, Show, Switch, createMemo, createSignal } from "solid-js";

import { Icon } from "~/components/Icon";
import { Wrap } from "~/components/Wrap";
import {
  hiddenKeyOf,
  nameCardTitle,
  takenLines,
  unifyResultLine,
  unifyViewOf,
  usedByLine,
  type NameCard as Card,
} from "~/domain/names";
import { messageOf, useApp, type UnifyModal } from "~/state/store";

/** One model the installs know under more than one name. */
export function NameCard(props: { card: Card }) {
  const app = useApp();
  const [pick, setPick] = createSignal(props.card.initial);

  const review = () => {
    const sha256 = props.card.sha256;
    const name = pick();
    app.setModal({ kind: "unify", sha256, name, plan: null, working: false, error: null });
    void planFor(app, sha256, name);
  };

  const keep = async () => {
    const key = hiddenKeyOf(props.card);
    const before = app.hiddenNameCards();
    const ok = await app.actions.run(() => app.engine.setHiddenNameCards([...before, key]));
    if (!ok) return;
    app.actions.showToast("Kept the names as they are.", "ok", {
      label: "Undo",
      run: () =>
        void app.actions.run(() =>
          app.engine.setHiddenNameCards(
            app
              .hiddenNameCards()
              .filter((h) => !(h.sha256 === key.sha256 && h.names.join("\n") === key.names.join("\n"))),
          ),
        ),
    });
  };

  return (
    <div class="nc">
      <div class="nc-h">{nameCardTitle(props.card)}</div>
      <div class="nc-s">Choose the name to use in every install.</div>
      <div class="nc-opts" role="radiogroup" aria-label="Name to use">
        <For each={props.card.names}>
          {(n) => (
            <button
              class="nc-opt"
              classList={{ on: pick() === n.name }}
              role="radio"
              aria-checked={pick() === n.name}
              onClick={() => setPick(n.name)}
            >
              <span class="radio" />
              <span class="nc-tx">
                <span class="nm" title={n.name}>
                  <Wrap text={n.name} />
                </span>
                <span class="who">{usedByLine(n, app.installs())}</span>
              </span>
            </button>
          )}
        </For>
      </div>
      <div class="nc-acts">
        <button class="btn pri" onClick={review}>
          Review name change
        </button>
        <button class="btn" onClick={() => void keep()}>
          Keep names as they are
        </button>
      </div>
    </div>
  );
}

type App = ReturnType<typeof useApp>;

/** Ask the engine what the change would do, and show it in the open dialog. */
async function planFor(app: App, sha256: string, name: string): Promise<void> {
  app.patchModal((m) => {
    if (m.kind === "unify") {
      m.working = true;
      m.error = null;
    }
  });
  try {
    const plan = await app.engine.planUnifyName(sha256, name);
    app.patchModal((m) => {
      if (m.kind === "unify" && m.sha256 === sha256 && m.name === name) {
        m.plan = plan;
        m.working = false;
      }
    });
  } catch (error) {
    app.patchModal((m) => {
      if (m.kind === "unify") {
        m.working = false;
        m.error = messageOf(error);
      }
    });
  }
}

/** The one dialog for giving a model one name, in whichever version the plan calls for. */
export function UnifyModalView() {
  const app = useApp();
  const modal = () => {
    const current = app.modal();
    return current && current.kind === "unify" ? current : null;
  };
  const view = createMemo(() => {
    const plan = modal()?.plan;
    return plan ? unifyViewOf(plan, app.installs()) : null;
  });
  const running = createMemo(() => {
    const v = view();
    return v?.kind === "running" ? v : null;
  });
  const confirm = createMemo(() => {
    const v = view();
    return v?.kind === "confirm" ? v : null;
  });
  const close = () => app.setModal(null);

  const use = async (current: UnifyModal) => {
    if (current.working) return;
    app.patchModal((m) => {
      if (m.kind === "unify") {
        m.working = true;
        m.error = null;
      }
    });
    try {
      const result = await app.engine.unifyName(current.sha256, current.name);
      if (result.stopped) {
        app.patchModal((m) => {
          if (m.kind === "unify") {
            m.working = false;
            m.error = result.stopped!.message;
          }
        });
        await app.actions.refresh();
        return;
      }
      app.setModal(null);
      app.setNameResult(unifyResultLine(result, app.installs()));
      await app.actions.refresh();
      // An install that kept its name still uses the other one, so the model
      // still has two names. The person has settled it: the card goes, and
      // comes back only if the installs start using yet another name.
      const left = app.nameCards().find((c) => c.sha256 === current.sha256);
      if (left) {
        await app.actions.run(() =>
          app.engine.setHiddenNameCards([...app.hiddenNameCards(), hiddenKeyOf(left)]),
        );
      }
    } catch (error) {
      app.patchModal((m) => {
        if (m.kind === "unify") {
          m.working = false;
          m.error = messageOf(error);
        }
      });
    }
  };

  return (
    <Show when={modal()}>
      {(current) => (
        <div
          class="veil"
          onClick={(e) => {
            if (e.target === e.currentTarget && !current().working) close();
          }}
        >
          <div class="modal" role="dialog" aria-modal="true" aria-labelledby="unify-h">
            <div class="mh">
              <h2 id="unify-h">
                <Switch fallback={<>Review name change</>}>
                  <Match when={running()}>{(v) => <>{v().heading}</>}</Match>
                  <Match when={confirm()}>
                    {(v) => (
                      <For each={v().heading}>
                        {(part) => (
                          <Show when={part.strong} fallback={<>{part.text}</>}>
                            <b class="emph">{part.text}</b>
                          </Show>
                        )}
                      </For>
                    )}
                  </Match>
                </Switch>
              </h2>
              <span class="sp" />
              <button class="tb-btn" aria-label="Close" disabled={current().working} onClick={close}>
                <Icon name="x" size={12} />
              </button>
            </div>
            <div class="mb">
              <Switch>
                <Match when={running()}>{(v) => <p class="mnote">{v().body}</p>}</Match>
                <Match when={confirm()}>
                  {(c) => {
                    return (
                      <>
                        <div class="fnbox det-name">
                          <Wrap text={c().name} />
                        </div>
                        <Show when={c().taken}>
                          {(taken) => (
                            <p class="note nc-aside">
                              {takenLines(taken())[0]}
                              <br />
                              {takenLines(taken())[1]}
                            </p>
                          )}
                        </Show>
                        <p class="mnote">
                          Saved workflows using{" "}
                          <For each={c().goingAway}>
                            {(n, i) => (
                              <>
                                {i() > 0 ? " or " : ""}
                                <span class="emph">
                                  <Wrap text={n} />
                                </span>
                              </>
                            )}
                          </For>
                          {c().inInstalls ? ` in ${c().inInstalls}` : ""} will show a missing
                          model until you pick this name.
                        </p>
                        <Switch>
                          <Match when={c().workflows === null}>
                            <p class="mnote">{c().method}</p>
                          </Match>
                          <Match when={c().workflows!.length === 0}>
                            <p class="mnote">
                              <b class="emph">No saved workflows to fix.</b>
                            </p>
                          </Match>
                          <Match when={c().workflows!.length > 0}>
                            <p class="mnote bright">Workflows to fix:</p>
                            <ul class="paths wfs">
                              <For each={c().workflows!}>{(w) => <li>• {w}</li>}</For>
                            </ul>
                          </Match>
                        </Switch>
                      </>
                    );
                  }}
                </Match>
              </Switch>
              <Show when={current().error}>
                {(message) => (
                  <div class="verdict no" role="alert">
                    <h4>
                      <Icon name="x" size={12} />
                      That did not happen
                    </h4>
                    <p>{message()}</p>
                  </div>
                )}
              </Show>
            </div>
            <div class="mf">
              <span class="sp" />
              <button class="btn" disabled={current().working} onClick={close}>
                Cancel
              </button>
              <Switch>
                <Match when={running()}>
                  <button
                    class="btn pri"
                    disabled={current().working}
                    onClick={() => void planFor(app, current().sha256, current().name)}
                  >
                    Check again
                  </button>
                </Match>
                <Match when={confirm()}>
                  {(v) => (
                    <Show when={v().cta}>
                      {(cta) => (
                        <button
                          class="btn pri"
                          disabled={current().working || app.anyBusy() !== null}
                          onClick={() => void use(current())}
                        >
                          {app.anyBusy() ?? cta()}
                        </button>
                      )}
                    </Show>
                  )}
                </Match>
              </Switch>
            </div>
          </div>
        </div>
      )}
    </Show>
  );
}
