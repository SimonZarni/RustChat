import { useCallback, useEffect, useState } from 'react'
import { api, errorMessage } from '../../lib/api'
import type { User } from '../../types'

export type Notification = {
  id: string
  kind: 'direct_message' | 'mention' | 'friend_request'
  actor_id: string | null
  actor_username: string | null
  conversation_id: string | null
  message_id: string | null
  created_at: string
  read_at: string | null
}

export function useNotifications(user: User | null, onError: (message: string) => void) {
  const [items, setItems] = useState<Notification[]>([])
  const [unreadCount, setUnreadCount] = useState(0)
  const refresh = useCallback(async () => {
    if (!user) { setItems([]); setUnreadCount(0); return }
    const result = await api<{ notifications: Notification[]; unread_count: number }>('/api/v1/notifications?limit=50')
    setItems(result.notifications)
    setUnreadCount(result.unread_count)
  }, [user?.id])

  useEffect(() => {
    if (!user) return
    void refresh().catch((cause: unknown) => onError(errorMessage(cause, 'Could not load notifications')))
    const onFocus = () => { void refresh().catch(() => {}) }
    window.addEventListener('focus', onFocus)
    return () => window.removeEventListener('focus', onFocus)
  }, [user?.id, refresh, onError])

  const markRead = useCallback(async (id: string) => {
    await api(`/api/v1/notifications/${id}/read`, { method: 'PUT' })
    await refresh()
  }, [refresh])
  const markAllRead = useCallback(async () => {
    await api('/api/v1/notifications/read-all', { method: 'PUT' })
    await refresh()
  }, [refresh])
  return { items, unreadCount, refresh, markRead, markAllRead }
}
