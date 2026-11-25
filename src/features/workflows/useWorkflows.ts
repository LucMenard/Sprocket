import { useQuery } from '@tanstack/react-query'
import { api } from '@/lib/api'

/** Workflow files are read again each time a screen that shows them opens. */
export const useWorkflows = () => useQuery({ queryKey: ['workflows'], queryFn: api.workflows })
