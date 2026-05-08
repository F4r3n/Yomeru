import { writable } from "svelte/store";
import type { KanjiEntry, WordEntry } from "../shared/types.ts";

interface PopupState {
  visible: boolean;
  entries: WordEntry[];
  kanjiEntries: KanjiEntry[];
  x: number;
  y: number;
  pinned: boolean;
  sentence: string;
}

let _pinned = false;

function createPopupStore() {
  const { subscribe, set, update } = writable<PopupState>({
    visible: false,
    entries: [],
    kanjiEntries: [],
    x: 0,
    y: 0,
    pinned: false,
    sentence: "",
  });

  return {
    subscribe,
    show(entries: WordEntry[], kanjiEntries: KanjiEntry[], x: number, y: number, sentence: string) {
      _pinned = false;
      set({ visible: true, entries, kanjiEntries, x, y, pinned: false, sentence });
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
