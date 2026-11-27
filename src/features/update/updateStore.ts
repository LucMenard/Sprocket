// Adapted from Tracket (MIT, Copyright (c) 2026 Hydhen): the same states, the same quiet automatic check.
import { invoke } from '@tauri-apps/api/core'
import { relaunch } from '@tauri-apps/plugin-process'
import { check, type Update } from '@tauri-apps/plugin-updater'
import { create } from 'zustand'
import { createJSONStorage, persist } from 'zustand/middleware'
import { useRuns } from '@/features/runs/useRuns'
import { inTauri } from '@/lib/api'

export type UpdateStatus =
  | { kind: 'idle' }
  | { kind: 'checking' }
  | { kind: 'upToDate' }
  | { kind: 'available'; update: Update }
  /** `percent` is null until the size of the download is known */
  | { kind: 'installing'; percent: number | null }
  /** `install`: the download or the installer failed, not the check */
  | { kind: 'error'; message: string; install?: boolean }

interface UpdateState {
  /** Whether to look for a new version each time the app starts */
  auto: boolean
  setAuto: (auto: boolean) => void
  status: UpdateStatus
  /** Version whose band was closed: only a newer one brings it back */
  dismissed: string | null
  dismiss: () => void
  /** `manual` checks report what they found, even nothing or a failure; automatic ones stay quiet */
  check: (manual: boolean) => Promise<void>
  install: () => Promise<void>
}

const messageOf = (error: unknown) => (error instanceof Error ? error.message : String(error))

/** A dev build must not replace itself with a release, and the browser preview has no updater. */
export const canUpdate = inTauri && !import.meta.env.DEV

export const useUpdates = create<UpdateState>()(
  persist(
    (set, get) => ({
      auto: true,
      setAuto: (auto) => set({ auto }),
      status: { kind: 'idle' },
      dismissed: null,
      dismiss: () => {
        const { status } = get()
        if (status.kind === 'available') set({ dismissed: status.update.version })
        else if (status.kind === 'error') set({ status: { kind: 'idle' } })
      },

      check: async (manual) => {
        const { kind } = get().status
        if (kind === 'checking' || kind === 'installing') return
        if (!canUpdate) {
          if (manual) set({ status: { kind: 'error', message: 'a development build does not update itself' } })
          return
        }
        if (manual) set({ status: { kind: 'checking' }, dismissed: null })
        try {
          const update = await check()
          set({ status: update ? { kind: 'available', update } : { kind: manual ? 'upToDate' : 'idle' } })
        } catch (error) {
          // Offline, or GitHub unreachable: an automatic check tries again next start
          set({ status: manual ? { kind: 'error', message: messageOf(error) } : { kind: 'idle' } })
        }
      },

      install: async () => {
        const { status } = get()
        if (status.kind !== 'available') return
        let total = 0
        let received = 0
        set({ status: { kind: 'installing', percent: null } })
        try {
          // The installer replaces the agent host's exe, so the agents stop first; their runs show as interrupted
          await invoke('stop_agents_for_update')
          // The download is checked against the public key in the app's configuration before it runs
          await status.update.downloadAndInstall((event) => {
            if (event.event === 'Started') total = event.data.contentLength ?? 0
            if (event.event === 'Progress') {
              received += event.data.chunkLength
              set({ status: { kind: 'installing', percent: total ? Math.min(100, Math.round((received / total) * 100)) : null } })
            }
          })
          await relaunch()
        } catch (error) {
          set({ status: { kind: 'error', message: messageOf(error), install: true } })
        }
      },
    }),
    {
      name: 'sprocket-updates',
      storage: createJSONStorage(() => localStorage),
      version: 1,
      partialize: (s) => ({ auto: s.auto }),
      merge: (persisted, current) => ({
        ...current,
        auto: (persisted as { auto?: unknown } | undefined)?.auto !== false,
      }),
    },
  ),
)

/** Runs that installing an update would stop. */
export function useRunsGoing() {
  return (useRuns().data ?? []).filter((r) => r.state === 'running' || r.state === 'waiting').length
}
