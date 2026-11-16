import { useCallback, useEffect, useLayoutEffect, useRef, useState, type MutableRefObject } from 'react'
import type { User } from '../../types'
import type { CommunityState } from '../communities/useCommunities'
import type { ConversationState } from '../messages/useConversation'

type RealtimeEvent = {
  type?: string
  event_id?: string
  conversation_id?: string
  sequence?: number
  occurred_at?: string
  payload?: {
    message_id?: string
    sender_id?: string
    client_message_id?: string
    reply_to_message_id?: string | null
    mentioned_user_ids?: string[]
    content?: string
    edited_at?: string | null
    deleted_at?: string | null
    user_id?: string
    sequence?: number
    emoji?: string
    online?: boolean | null
    community_id?: string
  }
}

const MESSAGE_EVENTS = ['message.created', 'message.updated', 'message.deleted']
const TYPING_EXPIRY_MS = 4500
const FIRST_RETRY_MS = 800
const MAX_RETRY_MS = 15000

type Params = {
  user: User | null
  activeId: string | null
  socketRef: MutableRefObject<WebSocket | null>
  conversation: ConversationState
  communities: CommunityState
}

/**
 * Owns the WebSocket: connects with bounded exponential backoff, resynchronizes
 * after every (re)connect, subscribes to the open conversation and routes
 * events into the conversation and community state. Events are only a
 * low-latency hint; the HTTP history is the source of truth.
 */
export function useRealtime({ user, activeId, socketRef, conversation, communities }: Params) {
  const [connected, setConnected] = useState(false)
  const [presence, setPresence] = useState<Record<string, boolean>>({})
  const [typingUserId, setTypingUserId] = useState<string | null>(null)
  const typingExpiryRef = useRef<number | null>(null)
  // The socket effect restarts only when the signed-in user changes, so it reads the latest state through this ref.
  const latest = useRef({ conversation, communities, userId: user?.id })
  useLayoutEffect(() => { latest.current = { conversation, communities, userId: user?.id } })

  const userId = user?.id
  useEffect(() => {
    if (!userId) return
    let stopped = false
    let retryTimer = 0
    let retryDelay = FIRST_RETRY_MS
    let socket: WebSocket

    const handleEvent = (data: RealtimeEvent) => {
      const { conversation: convo, communities: community } = latest.current
      const payload = data.payload
      const activeConversationId = convo.activeIdRef.current
      if ((data.type === 'members.updated' || data.type === 'channels.updated') && payload?.community_id) {
        const communityId = payload.community_id
        void community.loadCommunities().catch(() => {})
        if (data.type === 'channels.updated' && community.selectedCommunityIdRef.current === communityId) void community.reloadChannels(communityId).catch(() => {})
        if (data.type === 'members.updated' && community.selectedCommunityIdRef.current === communityId) community.bumpMembers()
        return
      }
      if (data.type === 'presence.updated' && payload?.user_id && payload.online !== undefined) {
        const { user_id: who, online } = payload
        setPresence((current) => {
          if (online === null) { const next = { ...current }; delete next[who]; return next }
          return { ...current, [who]: online }
        })
        return
      }
      if (data.type === 'delivery_ack' && data.sequence !== undefined && latest.current.userId) {
        convo.applyDeliveryCursor(latest.current.userId, data.sequence)
        return
      }
      if (data.type === 'delivery_cursor.updated' && data.conversation_id === activeConversationId && payload?.user_id && typeof payload.sequence === 'number') {
        convo.applyDeliveryCursor(payload.user_id, payload.sequence)
        return
      }
      if ((data.type === 'reaction.added' || data.type === 'reaction.removed') && data.event_id && data.conversation_id === activeConversationId && payload?.message_id && payload.emoji && payload.user_id) {
        convo.applyReactionEvent(data.event_id, payload.message_id, payload.emoji, data.type === 'reaction.added')
        return
      }
      if (data.type === 'read_cursor.updated' && data.conversation_id === activeConversationId && payload?.user_id && typeof payload.sequence === 'number') {
        convo.applyReadCursor(payload.user_id, payload.sequence)
        return
      }
      if ((data.type === 'typing.started' || data.type === 'typing.stopped') && data.conversation_id === activeConversationId && payload?.user_id) {
        if (typingExpiryRef.current !== null) window.clearTimeout(typingExpiryRef.current)
        setTypingUserId(data.type === 'typing.started' ? payload.user_id : null)
        if (data.type === 'typing.started') typingExpiryRef.current = window.setTimeout(() => setTypingUserId(null), TYPING_EXPIRY_MS)
        return
      }
      if (!MESSAGE_EVENTS.includes(data.type ?? '') || !payload?.message_id || !payload.sender_id || !payload.client_message_id || payload.content === undefined || !data.conversation_id || data.conversation_id !== activeConversationId) return
      convo.mergeMessages([{
        id: payload.message_id,
        conversation_id: data.conversation_id,
        sender_id: payload.sender_id,
        sequence: data.sequence ?? 0,
        client_message_id: payload.client_message_id,
        reply_to_message_id: payload.reply_to_message_id,
        mentioned_user_ids: payload.mentioned_user_ids ?? [],
        content: payload.content,
        created_at: data.occurred_at ?? new Date().toISOString(),
        edited_at: payload.edited_at,
        deleted_at: payload.deleted_at,
      }])
      if (data.sequence && data.sequence > 0) { convo.recordDelivered(data.sequence); void convo.markRead(data.sequence) }
    }

    const connect = () => {
      if (stopped) return
      const scheme = window.location.protocol === 'https:' ? 'wss:' : 'ws:'
      socket = new WebSocket(`${scheme}//${window.location.host}/api/v1/realtime`)
      socketRef.current = socket
      socket.onopen = async () => {
        retryDelay = FIRST_RETRY_MS
        setConnected(true)
        const { conversation: convo, communities: community } = latest.current
        void community.loadCommunities().catch(() => {})
        const communityId = community.selectedCommunityIdRef.current
        if (communityId) {
          void community.reloadChannels(communityId).catch(() => {})
          community.bumpMembers()
        }
        const conversationId = convo.activeIdRef.current
        if (!conversationId) return
        await convo.resync()
        if (convo.activeIdRef.current === conversationId && socket.readyState === WebSocket.OPEN) {
          socket.send(JSON.stringify({ type: 'subscribe', conversation_id: conversationId }))
        }
      }
      socket.onmessage = (event) => {
        try { handleEvent(JSON.parse(event.data) as RealtimeEvent) } catch { /* Ignore malformed frames. */ }
      }
      socket.onclose = () => {
        setConnected(false)
        setTypingUserId(null)
        if (!stopped) { retryTimer = window.setTimeout(connect, retryDelay); retryDelay = Math.min(retryDelay * 2, MAX_RETRY_MS) }
      }
      socket.onerror = () => socket.close()
    }
    connect()
    return () => {
      stopped = true
      window.clearTimeout(retryTimer)
      if (typingExpiryRef.current !== null) window.clearTimeout(typingExpiryRef.current)
      socket?.close()
      socketRef.current = null
      setConnected(false)
      setPresence({})
    }
  }, [userId, socketRef])

  // Subscribe to the open conversation (also when the socket becomes ready).
  useEffect(() => {
    const socket = socketRef.current
    if (!socket || socket.readyState !== WebSocket.OPEN) return
    if (activeId) socket.send(JSON.stringify({ type: 'subscribe', conversation_id: activeId }))
  }, [activeId, connected, socketRef])

  const sendTyping = useCallback((active: boolean) => {
    const socket = socketRef.current
    if (activeId && socket?.readyState === WebSocket.OPEN) {
      socket.send(JSON.stringify({ type: 'typing', conversation_id: activeId, active }))
    }
  }, [activeId, socketRef])

  return { connected, presence, typingUserId, sendTyping }
}
