import { get, writable } from "svelte/store";
import type { KanjiEntry, WordEntry } from "../shared/types.ts";

interface PopupState {
  visible: boolean;
  entries: WordEntry[];
  kanjiEntries: KanjiEntry[];
  wx1: number;
  wx2: number;
  wy1: number;
  wy2: number;
  pinned: boolean;
}

function createPopupStore() {
  const store = writable<PopupState>({
    visible: false,
    entries: [],
    kanjiEntries: [],
    wx1: 0,
    wx2: 0,
    wy1: 0,
    wy2: 0,
    pinned: false,
  });
  const { subscribe, set, update } = store;

  return {
    subscribe,
    show(
      entries: WordEntry[],
      kanjiEntries: KanjiEntry[],
      wx1: number,
      wx2: number,
      wy1: number,
      wy2: number,
    ) {
      set({ visible: true, entries, kanjiEntries, wx1, wx2, wy1, wy2, pinned: false });
    },
    pin() {
      update((s) => ({ ...s, pinned: true }));
    },
    hide() {
      if (get(store).pinned) return;
      update((s) => ({ ...s, visible: false }));
    },
    forceHide() {
      update((s) => ({ ...s, visible: false, pinned: false }));
    },
    isPinned() {
      return get(store).pinned;
    },
  };
}

export const popupStore = createPopupStore();
