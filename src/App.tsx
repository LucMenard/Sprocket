import { AppHeader } from '@/components/AppHeader'
import { AgentsView } from '@/features/agents/AgentsView'
import { IntegrationsView } from '@/features/integrations/IntegrationsView'
import { OverviewView } from '@/features/overview/OverviewView'
import { RunsView } from '@/features/runs/RunsView'
import { useRuns, useRunsLive } from '@/features/runs/useRuns'
import { WorkflowsView } from '@/features/workflows/WorkflowsView'
import { useNav, type ViewId } from '@/lib/nav'

const VIEWS: Record<ViewId, () => React.JSX.Element | null> = {
  overview: OverviewView,
  runs: RunsView,
  workflows: WorkflowsView,
  agents: AgentsView,
  integrations: IntegrationsView,
}

export default function App() {
  useRunsLive()
  const view = useNav((s) => s.view)
  const View = VIEWS[view]
  const waiting = (useRuns().data ?? []).reduce((n, r) => n + r.approvals.length, 0)

  return (
    <div className="flex h-screen flex-col bg-ct-page text-ct-text">
      <AppHeader waiting={waiting} />
      <main className="min-h-0 flex-1 overflow-y-auto">
        <View />
      </main>
    </div>
  )
}
