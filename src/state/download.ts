/**
 * What the Download screen knows: the list of downloads, which the engine
 * reports as it works, and the plan card for the address being read.
 *
 * It lives beside the rest of the store rather than inside the screen, so a
 * card the person was reading is still there when they come back to it.
 */

import { batch, type Accessor } from "solid-js";
import { createStore, reconcile, type SetStoreFunction } from "solid-js/store";

import type { Engine } from "~/ipc/contract";
import type { AddressPlan, AddressRefusal, Download } from "~/ipc/draft";

export interface DownloadCard {
  /** What is in the address field. */
  address: string;
  /** A read is under way and its answer is still to come. */
  reading: boolean;
  /** The engine's plan for the address, or null before one. */
  plan: AddressPlan | null;
  /** Why the site will not hand the file over, found while reading. */
  refusal: AddressRefusal | null;
  /** The engine refused the call itself, in its words. */
  failure: string | null;
  /** The folder the person chose, or null to take the engine's suggestion. */
  category: string | null;
  /** The installs ticked on the card. Held ones are never in it. */
  ticked: readonly string[];
  /** Download was pressed and the engine has not answered yet. */
  starting: boolean;
}

export interface DownloadState {
  readonly downloads: Accessor<readonly Download[]>;
  readonly card: DownloadCard;
  readonly setCard: SetStoreFunction<DownloadCard>;
  /** Replace the list with what the engine holds, for a start or a restart. */
  load(): Promise<void>;
  /** Replace the list with records already read. */
  replace(records: readonly Download[]): void;
  /** Take in one record the engine sent, new or changed. */
  receive(record: Download): void;
  /** Take a record off the screen once the engine let it go. */
  forget(downloadId: string): void;
  /**
   * Read an address, or read it again with another version, file or folder.
   * A later read wins: an answer that arrives after a newer read, or after
   * Cancel, is dropped.
   */
  read(
    address: string,
    choice?: { versionId?: number; fileId?: number; category?: string },
  ): Promise<void>;
  /** Forget the card. The address stays in the field. */
  cancel(): void;
}

const EMPTY_CARD: DownloadCard = {
  address: "",
  reading: false,
  plan: null,
  refusal: null,
  failure: null,
  category: null,
  ticked: [],
  starting: false,
};

/** The installs ticked on a new card, as the engine proposes them. */
export function defaultTicks(plan: AddressPlan): string[] {
  return plan.installs.filter((i) => i.state === "free" && i.ticked).map((i) => i.installId);
}

export function createDownloadState(
  engine: Engine,
  messageOf: (error: unknown) => string,
): DownloadState {
  // Kept by id, so a row that changes is updated in place rather than drawn
  // again, and a button under the person's pointer stays the same button.
  const [held, setHeld] = createStore<{ list: Download[] }>({ list: [] });
  const downloads = () => held.list;
  const setDownloads = (next: (list: readonly Download[]) => readonly Download[]) =>
    setHeld("list", reconcile([...next(held.list)], { key: "downloadId" }));
  const [card, setCard] = createStore<DownloadCard>({ ...EMPTY_CARD });
  let latestRead = 0;

  const receive = (record: Download) => {
    setDownloads((list) => {
      const at = list.findIndex((r) => r.downloadId === record.downloadId);
      if (at < 0) return [...list, record];
      const next = [...list];
      next[at] = record;
      return next;
    });
  };

  return {
    downloads,
    card,
    setCard,
    async load() {
      const list = await engine.listDownloads();
      setDownloads(() => list);
    },
    replace: (records) => setDownloads(() => records),
    receive,
    forget(downloadId) {
      setDownloads((list) => list.filter((r) => r.downloadId !== downloadId));
    },
    async read(address, choice = {}) {
      const id = ++latestRead;
      // A new address starts a new card. Another version, file or folder of
      // the same one keeps the ticks the person made.
      const sameCard =
        Object.keys(choice).length > 0 && card.plan !== null && card.address === address;
      batch(() => {
        setCard({ address, reading: true, failure: null });
        if (!sameCard) {
          setCard({ plan: null, refusal: null, ticked: [], category: null, starting: false });
        }
      });
      try {
        const reading = await engine.readModelAddress({ address, ...choice });
        if (id !== latestRead) return;
        if (reading.refusal) {
          setCard({ reading: false, plan: null, refusal: reading.refusal, ticked: [] });
          return;
        }
        const plan = reading.plan;
        let ticked: string[];
        if (sameCard) {
          const free = new Set(
            plan.installs.filter((i) => i.state === "free").map((i) => i.installId),
          );
          ticked = card.ticked.filter((t) => free.has(t));
        } else {
          ticked = defaultTicks(plan);
        }
        setCard({ plan, refusal: null, reading: false, ticked });
      } catch (error) {
        if (id !== latestRead) return;
        setCard({ reading: false, plan: null, refusal: null, failure: messageOf(error) });
      }
    },
    cancel() {
      latestRead += 1;
      setCard({
        reading: false,
        plan: null,
        refusal: null,
        failure: null,
        category: null,
        ticked: [],
        starting: false,
      });
    },
  };
}
