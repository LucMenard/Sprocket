import { create } from 'zustand'

/** Which run the Runs screen shows; the Overview sets it when a run is clicked there. */
export const useRunSelection = create<{ selected: string | null; select: (id: string | null) => void }>((set) => ({
  selected: null,
  select: (selected) => set({ selected }),
}))
