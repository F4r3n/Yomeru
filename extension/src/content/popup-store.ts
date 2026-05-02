import { writable } from "svelte/store";
import type { WordEntry } from "../shared/types.ts";

interface PopupState {
  visible: boolean;
  entries: WordEntry[];
  x: number;
  y: number;
  pinned: boolean;
}

let _pinned = false;

function createPopupStore() {
  const { subscribe, set, update } = writable<PopupState>({
    visible: false,
    entries: [],
    x: 0,
    y: 0,
    pinned: false,
  });

  return {
    subscribe,
    show(entries: WordEntry[], x: number, y: number) {
      _pinned = false;
      set({ visible: true, entries, x, y, pinned: false });
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
