import { writable } from "svelte/store";
import type { WordEntry } from "../shared/types.ts";

interface PopupState {
  visible: boolean;
  entries: WordEntry[];
  x: number;
  y: number;
}

function createPopupStore() {
  const { subscribe, set, update } = writable<PopupState>({
    visible: false,
    entries: [],
    x: 0,
    y: 0,
  });

  return {
    subscribe,
    show(entries: WordEntry[], x: number, y: number) {
      set({ visible: true, entries, x, y });
    },
    hide() {
      update((s) => ({ ...s, visible: false }));
    },
  };
}

export const popupStore = createPopupStore();
