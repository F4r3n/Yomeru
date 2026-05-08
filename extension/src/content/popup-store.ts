import { writable } from "svelte/store";
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

let _pinned = false;

function createPopupStore() {
  const { subscribe, set, update } = writable<PopupState>({
    visible: false,
    entries: [],
    kanjiEntries: [],
    wx1: 0,
    wx2: 0,
    wy1: 0,
    wy2: 0,
    pinned: false,
  });

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
      _pinned = false;
      set({ visible: true, entries, kanjiEntries, wx1, wx2, wy1, wy2, pinned: false });
    },
    pin() {
      _pinned = true;
      update((s) => ({ ...s, pinned: true }));
    },
    hide() {
      if (_pinned) return;
      update((s) => ({ ...s, visible: false }));
    },
    forceHide() {
      _pinned = false;
      update((s) => ({ ...s, visible: false, pinned: false }));
    },
    isPinned() {
      return _pinned;
    },
  };
}

export const popupStore = createPopupStore();
