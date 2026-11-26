import { useQuery, useQueryClient } from '@tanstack/react-query'
import { listen } from '@tauri-apps/api/event'
import { useEffect } from 'react'
import { api, inTauri } from '@/lib/api'
import { useNav } from '@/lib/nav'
import { useRunSelection } from './useRunSelection'

/** Every run, reloaded whenever the engine says something changed. */
export function useRuns() {
  return useQuery({ queryKey: ['runs'], queryFn: () => api.runs() })
}

/** Mounted once: turns the engine's "runs-changed" events into reloads. */
export function useRunsLive() {
  const queryClient = useQueryClient()
  useEffect(() => {
    if (!inTauri) return
    const unlisten = listen('runs-changed', () => {
      void queryClient.invalidateQueries({ queryKey: ['runs'] })
      void queryClient.invalidateQueries({ queryKey: ['job-messages'] })
    })
    // Tracket's "Open run": the window is already up, show that run
    const unshow = listen<string>('show-run', (event) => {
      void queryClient.invalidateQueries({ queryKey: ['runs'] })
      useRunSelection.getState().select(event.payload)
      useNav.getState().go('runs')
    })
    return () => {
      void unlisten.then((off) => off())
      void unshow.then((off) => off())
    }
  }, [queryClient])
}
