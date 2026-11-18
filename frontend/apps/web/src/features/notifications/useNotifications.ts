import { useCallback, useEffect, useRef, useState } from 'react'
import { api, errorMessage } from '../../lib/api'
import type { User } from '../../types'

export type Notification = {
  id: string
  kind: 'direct_message' | 'mention' | 'friend_request' | 'community_event'
  actor_id: string | null
  actor_username: string | null
  conversation_id: string | null
  message_id: string | null
  community_id: string | null
  event_type: 'member_joined' | 'member_left' | 'member_removed' | 'channel_created' | 'channel_updated' | 'channel_deleted' | null
  created_at: string
  read_at: string | null
}

export function useNotifications(user: User | null, onError: (message: string) => void) {
  const [items, setItems] = useState<Notification[]>([])
  const [unreadCount, setUnreadCount] = useState(0)
  const priorUnread = useRef<number | null>(null)
  const dndSchedule = useRef<{ enabled: boolean; start_time: string; end_time: string; timezone: string; weekdays: number[] } | null>(null)
  const inQuietHours = () => {
    const schedule = dndSchedule.current
    if (!schedule?.enabled) return false
    try {
      const parts = new Intl.DateTimeFormat('en-US', { timeZone: schedule.timezone, weekday: 'short', hour: '2-digit', minute: '2-digit', hourCycle: 'h23' }).formatToParts(new Date())
      const day = ['Mon','Tue','Wed','Thu','Fri','Sat','Sun'].indexOf(parts.find((part) => part.type === 'weekday')?.value ?? '') + 1
      const now = Number(parts.find((part) => part.type === 'hour')?.value) * 60 + Number(parts.find((part) => part.type === 'minute')?.value)
      const start = Number(schedule.start_time.slice(0, 2)) * 60 + Number(schedule.start_time.slice(3))
      const end = Number(schedule.end_time.slice(0, 2)) * 60 + Number(schedule.end_time.slice(3))
      if (start < end) return schedule.weekdays.includes(day) && now >= start && now < end
      const previousDay = day === 1 ? 7 : day - 1
      return (schedule.weekdays.includes(day) && now >= start) || (schedule.weekdays.includes(previousDay) && now < end)
    } catch { return false }
  }
  const refresh = useCallback(async () => {
    if (!user) { setItems([]); setUnreadCount(0); return }
    const result = await api<{ notifications: Notification[]; unread_count: number }>('/api/v1/notifications?limit=50')
    if (priorUnread.current !== null && result.unread_count > priorUnread.current && document.visibilityState === 'hidden' && localStorage.getItem('rustchat.browserNotifications') === 'true' && 'Notification' in window && Notification.permission === 'granted' && !inQuietHours()) {
      new Notification('RustChat', { body: 'You have a new notification.', icon: '/favicon.ico', tag: 'rustchat-unread' })
    }
    priorUnread.current = result.unread_count
    setItems(result.notifications)
    setUnreadCount(result.unread_count)
  }, [user?.id])

  useEffect(() => {
    if (!user) { priorUnread.current = null; dndSchedule.current = null; return }
    void api<typeof dndSchedule.current & {}>('/api/v1/notifications/dnd-schedule').then((schedule) => { dndSchedule.current = schedule }).catch(() => {})
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
