import { useQuery } from '@tanstack/react-query'
import { api } from './api'

/** The app's state: version, the local API, the agent host. Polled, as the host may still be starting. */
export const useStatus = () => useQuery({ queryKey: ['status'], queryFn: api.status, refetchInterval: 3000 })

/** Recent activity across runs; reloaded with the runs (see useRunsLive). */
export const useActivity = (limit = 30) =>
  useQuery({ queryKey: ['runs', 'activity', limit], queryFn: () => api.activity(limit) })

export const useMcpServers = () => useQuery({ queryKey: ['runs', 'mcp'], queryFn: api.mcpServers })

/** Calls to the local API come from outside the app, so they are polled. */
export const useApiCalls = () => useQuery({ queryKey: ['api-calls'], queryFn: api.apiCalls, refetchInterval: 5000 })

/** Tracket counts as connected when it called within this long. */
const CONNECTED_WITHIN_MS = 10 * 60_000

/** The last successful call to the local API, and whether it is recent enough to call Tracket connected. */
export function useTracketConnection() {
  const { data, dataUpdatedAt } = useApiCalls()
  const calls = data ?? []
  const lastOk = calls.find((c) => c.status < 400)
  // Measured against when the calls were fetched, so rendering stays pure
  const connected = !!lastOk && dataUpdatedAt - lastOk.at < CONNECTED_WITHIN_MS
  return { calls, lastOk, connected }
}
