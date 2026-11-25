import { create } from 'zustand'

export type ViewId = 'overview' | 'runs' | 'workflows' | 'agents' | 'integrations'

const VIEWS: ViewId[] = ['overview', 'runs', 'workflows', 'agents', 'integrations']

/** `#runs` opens Runs, so a link (or the debug build's SPROCKET_VIEW) can say which screen to show. */
function fromHash(): ViewId | null {
  const view = window.location.hash.slice(1) as ViewId
  return VIEWS.includes(view) ? view : null
}

/** The screen shown; any screen can send you to another. */
export const useNav = create<{ view: ViewId; go: (view: ViewId) => void }>((set) => ({
  view: fromHash() ?? 'overview',
  go: (view) => set({ view }),
}))

window.addEventListener('hashchange', () => {
  const view = fromHash()
  if (view) useNav.getState().go(view)
})
