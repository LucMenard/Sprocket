import { useQuery } from '@tanstack/react-query'
import { api } from '@/lib/api'

/** The shared agent types (agents/*.yaml), read again whenever a screen that shows them opens. */
export const useAgents = () => useQuery({ queryKey: ['agents'], queryFn: api.agents })
