import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react'
import { api, errorMessage } from '../../lib/api'
import type { Category, Channel, Community, User } from '../../types'

/** Community, channel and category state for the signed-in user, kept in sync with the server. */
export function useCommunities(user: User | null, onError: (message: string) => void) {
  const [communities, setCommunities] = useState<Community[]>([])
  const [selectedCommunityId, setSelectedCommunityId] = useState<string | null>(null)
  const [channels, setChannels] = useState<Channel[]>([])
  const [categories, setCategories] = useState<Category[]>([])
  /** Incremented when the member list may have changed, so an open management dialog reloads it. */
  const [membersVersion, setMembersVersion] = useState(0)
  const selectedCommunityIdRef = useRef<string | null>(selectedCommunityId)
  useLayoutEffect(() => { selectedCommunityIdRef.current = selectedCommunityId })

  const applyCommunities = useCallback((list: Community[]) => {
    setCommunities(list)
    setSelectedCommunityId((current) => current && list.some((item) => item.id === current) ? current : list[0]?.id ?? null)
  }, [])

  const loadCommunities = useCallback(async () => {
    const result = await api<{ communities: Community[] }>('/api/v1/communities')
    applyCommunities(result.communities)
  }, [applyCommunities])

  const reloadChannels = useCallback(async (communityId: string) => {
    const [channelResult, categoryResult] = await Promise.all([
      api<{ channels: Channel[] }>(`/api/v1/communities/${communityId}/channels`),
      api<{ categories: Category[] }>(`/api/v1/communities/${communityId}/categories`),
    ])
    setChannels(channelResult.channels)
    setCategories(categoryResult.categories)
  }, [])

  const bumpMembers = useCallback(() => setMembersVersion((version) => version + 1), [])

  const userId = user?.id
  useEffect(() => {
    if (!userId) return
    let alive = true
    api<{ communities: Community[] }>('/api/v1/communities')
      .then((result) => { if (alive) applyCommunities(result.communities) })
      .catch((cause: unknown) => { if (alive) onError(errorMessage(cause, 'Could not load communities')) })
    return () => { alive = false }
  }, [userId, applyCommunities, onError])

  useEffect(() => {
    if (!selectedCommunityId) return
    let alive = true
    Promise.all([
      api<{ channels: Channel[] }>(`/api/v1/communities/${selectedCommunityId}/channels`),
      api<{ categories: Category[] }>(`/api/v1/communities/${selectedCommunityId}/categories`),
    ])
      .then(([channelResult, categoryResult]) => { if (alive) { setChannels(channelResult.channels); setCategories(categoryResult.categories) } })
      .catch((cause: unknown) => { if (alive) onError(errorMessage(cause, 'Could not load channels')) })
    return () => { alive = false }
  }, [selectedCommunityId, onError])

  const reset = useCallback(() => {
    setCommunities([]); setSelectedCommunityId(null); setChannels([]); setCategories([])
  }, [])

  return {
    communities, setCommunities,
    selectedCommunityId, setSelectedCommunityId, selectedCommunityIdRef,
    channels, setChannels,
    categories, setCategories,
    membersVersion, bumpMembers,
    loadCommunities, reloadChannels, reset,
  }
}

export type CommunityState = ReturnType<typeof useCommunities>
