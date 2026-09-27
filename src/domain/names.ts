/**
 * Cleanup's card for one model that the installs know under more than one
 * name, and the dialog that gives it one name everywhere.
 *
 * Everything here is worked out from what the engine returns. The engine
 * decides what happens to each link; this file only says it.
 */

import { leafOf } from "~/domain/format";
import { installNameOf } from "~/domain/installname";
import { numberWord } from "~/domain/view";
import type {
  HiddenNameCard,
  Install,
  NameGroup,
  UnifyPlan,
  UnifyResult,
} from "~/ipc/contract";

type Nameable = Pick<Install, "id" | "label" | "root">;

export interface NameCardName {
  name: string;
  /** The installs whose links carry this name. */
  installIds: readonly string[];
  links: number;
}

export interface NameCard {
  sha256: string;
  /** Only names an install uses, in the engine's order. */
  names: readonly NameCardName[];
  /** The name picked when the card first shows. */
  initial: string;
}

/** The names a hidden card had, so a new name brings the card back. */
export function hiddenKeyOf(card: Pick<NameCard, "sha256" | "names">): HiddenNameCard {
  return { sha256: card.sha256, names: card.names.map((n) => n.name).sort() };
}

function isHidden(key: HiddenNameCard, hidden: readonly HiddenNameCard[]): boolean {
  return hidden.some(
    (h) =>
      h.sha256 === key.sha256 &&
      h.names.length === key.names.length &&
      [...h.names].sort().every((name, i) => name === key.names[i]),
  );
}

/** One card for each model whose installs use two or more names for it. */
export function nameCardsOf(
  groups: readonly NameGroup[],
  hidden: readonly HiddenNameCard[],
): NameCard[] {
  const cards: NameCard[] = [];
  for (const group of groups) {
    const names = group.names
      .filter((n) => n.seenInInstalls.length > 0)
      .map((n) => ({ name: n.name, installIds: n.seenInInstalls, links: n.usedByLinks }));
    if (names.length < 2) continue;
    const card = { sha256: group.sha256, names };
    if (isHidden(hiddenKeyOf(card), hidden)) continue;
    // The name the most installs use, then the most links, then the longer.
    const initial = [...names].sort(
      (a, b) =>
        b.installIds.length - a.installIds.length ||
        b.links - a.links ||
        b.name.length - a.name.length,
    )[0]!.name;
    cards.push({ ...card, initial });
  }
  return cards;
}

/** "ONE MODEL, TWO NAMES" */
export function nameCardTitle(card: NameCard): string {
  return `ONE MODEL, ${numberWord(card.names.length).toUpperCase()} NAMES`;
}

/** "ComfyUI-Beta · ComfyUI-Alpha" */
export function usedByLine(name: NameCardName, installs: readonly Nameable[]): string {
  return name.installIds.map((id) => installNameOf(id, installs)).join(" · ");
}

/** The top bar's part for these cards, or null when none shows. */
export function nameCardsSummary(cards: readonly NameCard[]): string | null {
  if (cards.length === 0) return null;
  if (cards.length === 1) {
    return `1 model has ${numberWord(cards[0]!.names.length)} names in your installs.`;
  }
  return `${cards.length} models have more than one name in your installs.`;
}

function joinAnd(items: readonly string[]): string {
  if (items.length <= 1) return items.join("");
  return `${items.slice(0, -1).join(", ")} and ${items[items.length - 1]}`;
}

function distinct(ids: readonly string[]): string[] {
  return [...new Set(ids)];
}

/** What the dialog says, for the plan the engine made. */
export type UnifyView =
  | {
      kind: "running";
      heading: string;
      body: string;
    }
  | {
      kind: "confirm";
      /** The heading, with the installs that change marked when a name is taken. */
      heading: readonly { text: string; strong?: boolean }[];
      name: string;
      /** The installs that keep their name because it is taken there, with its sentences. */
      taken: readonly string[] | null;
      /** The names that go away. */
      goingAway: readonly string[];
      /** "in ComfyUI-Beta", when only some installs change. */
      inInstalls: string | null;
      /** The workflows to fix, or null when no saved workflow was searched. */
      workflows: readonly string[] | null;
      /** What the search did, shown when nothing was searched. */
      method: string | null;
      /** Null when no install can take the name. */
      cta: string | null;
    };

export function unifyViewOf(plan: UnifyPlan, installs: readonly Nameable[]): UnifyView {
  const nameOf = (id: string) => installNameOf(id, installs);

  if (plan.running.length > 0) {
    const who = joinAnd(plan.running.map(nameOf));
    return {
      kind: "running",
      heading: `Close ${who} first`,
      body: `The name can change after ${who} ${plan.running.length === 1 ? "is" : "are"} closed.`,
    };
  }

  const all = distinct(plan.links.map((s) => s.installId));
  const changingIds = distinct(
    plan.links.filter((s) => s.action === "rename" || s.action === "remove").map((s) => s.installId),
  );
  const takenIds = distinct(
    plan.links.filter((s) => s.action === "blockedTaken").map((s) => s.installId),
  );
  const goingAway = distinct(
    plan.links.filter((s) => s.linkName !== plan.name).map((s) => s.linkName),
  );

  const results = plan.workflows.filter((w) => goingAway.includes(w.name));
  const searched = results.length === 0 || results.some((w) => w.searched);
  const workflows = searched
    ? distinct(
        results
          .flatMap((w) => w.matches)
          .filter((m) => changingIds.includes(m.installId))
          // The engine sends the name without .json. The file is what the person opens.
          .map((m) => leafOf(m.workflowPath)),
      )
    : null;
  const method = searched ? null : (results[0]?.method ?? null);

  if (takenIds.length > 0) {
    const changing = joinAnd(changingIds.map(nameOf));
    return {
      kind: "confirm",
      heading:
        changingIds.length > 0
          ? [{ text: "Use this name in " }, { text: changing, strong: true }, { text: "?" }]
          : [{ text: "Use this name?" }],
      name: plan.name,
      taken: takenIds.map(nameOf),
      goingAway,
      inInstalls: changingIds.length > 0 ? changing : null,
      workflows,
      method,
      cta: changingIds.length > 0 ? `Change name in ${changing}` : null,
    };
  }

  return {
    kind: "confirm",
    heading: [
      {
        text:
          all.length === 2
            ? "Use this name in both installs?"
            : all.length === 1
              ? `Use this name in ${nameOf(all[0]!)}?`
              : `Use this name in all ${all.length} installs?`,
      },
    ],
    name: plan.name,
    taken: null,
    goingAway,
    inInstalls: null,
    workflows,
    method,
    cta: "Use this name",
  };
}

/** The sentences under the name when some installs keep theirs. */
export function takenLines(taken: readonly string[]): [string, string] {
  const who = joinAnd(taken);
  return taken.length === 1
    ? [`${who} already uses this name for another file.`, "Its model name will stay as it is."]
    : [`${who} already use this name for another file.`, "Their model names will stay as they are."];
}

/** Cleanup's line once the name has changed. */
export function unifyResultLine(result: UnifyResult, installs: readonly Nameable[]): string {
  const nameOf = (id: string) => installNameOf(id, installs);
  const changed = distinct([...result.renamed, ...result.removed].map((s) => s.installId));
  const kept = distinct(result.skipped.map((s) => s.installId)).filter(
    (id) => !changed.includes(id),
  );
  if (kept.length > 0) {
    return `Name changed in ${joinAnd(changed.map(nameOf))}. ${joinAnd(kept.map(nameOf))} kept ${kept.length === 1 ? "its name" : "their names"}.`;
  }
  if (changed.length === 2) return "Name changed in both installs.";
  if (changed.length === 1) return `Name changed in ${nameOf(changed[0]!)}.`;
  return `Name changed in all ${changed.length} installs.`;
}
