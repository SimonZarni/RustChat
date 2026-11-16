import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type MutableRefObject } from 'react'
import { api, errorMessage } from '../../lib/api'
import { mergeBySequence } from '../../lib/format'
import { uuid7 } from '../../lib/ids'
import { useDialogs } from '../../components/dialogs-context'
import type { Message, MessageReaction, ReadCursor, User } from '../../types'

const toCursorMap = (cursors: ReadCursor[]) => Object.fromEntries(cursors.map((cursor) => [cursor.user_id, cursor.sequence]))

/**
 * Everything about the open conversation: ordered messages, reactions, read and
 * delivery positions and the writes that change them. Realtime events feed the
 * same state through the returned `apply*` functions.
 */
export function useConversation(
  activeId: string | null,
  socketRef: MutableRefObject<WebSocket | null>,
  onError: (message: string) => void,
) {
  const dialogs = useDialogs()
  const [messages, setMessages] = useState<Message[]>([])
  const [loadedConversationId, setLoadedConversationId] = useState<string | null>(null)
  const [older, setOlder] = useState<{ conversationId: string | null; cursor: number | null }>({ conversationId: null, cursor: null })
  const [loadingOlder, setLoadingOlder] = useState(false)
  const [reply, setReplyTo] = useState<Message | null>(null)
  const [profiles, setProfiles] = useState<Record<string, User>>({})
  const [reactions, setReactions] = useState<Record<string, MessageReaction[]>>({})
  const [pendingReactions, setPendingReactions] = useState<Record<string, boolean>>({})
  const [readCursors, setReadCursors] = useState<Record<string, number>>({})
  const [deliveryCursors, setDeliveryCursors] = useState<Record<string, number>>({})
  const activeIdRef = useRef<string | null>(activeId)
  useLayoutEffect(() => { activeIdRef.current = activeId })
  // Both belong to the conversation they were set in; derive instead of resetting in an effect.
  const olderCursor = older.conversationId === activeId ? older.cursor : null
  const replyTo = reply?.conversation_id === activeId ? reply : null
  const seenReactionEventIds = useRef(new Set<string>())

  const mergeMessages = useCallback((incoming: Message[]) => {
    if (!incoming.length) return
    setMessages((current) => mergeBySequence(current, incoming))
  }, [])

  const replaceReactionsForMessages = useCallback((messageIds: string[], incoming: MessageReaction[]) => {
    setReactions((current) => {
      const next = { ...current }
      for (const id of messageIds) delete next[id]
      for (const reaction of incoming) next[reaction.message_id] = [...(next[reaction.message_id] ?? []), reaction]
      return next
    })
  }, [])

  const updateReactionCount = useCallback((messageId: string, emoji: string, added: boolean) => {
    setReactions((current) => {
      const messageReactions = [...(current[messageId] ?? [])]
      const index = messageReactions.findIndex((reaction) => reaction.emoji === emoji)
      if (index < 0 && added) {
        messageReactions.push({ message_id: messageId, emoji, count: 1, reacted_by_me: false })
      } else if (index >= 0) {
        const existing = messageReactions[index]!
        const count = existing.count + (added ? 1 : -1)
        if (count <= 0) messageReactions.splice(index, 1)
        else messageReactions[index] = { ...existing, count }
      }
      return { ...current, [messageId]: messageReactions }
    })
  }, [])

  const markRead = useCallback(async (sequence: number) => {
    const conversationId = activeIdRef.current
    if (!conversationId || sequence <= 0) return
    try {
      const result = await api<{ cursor: ReadCursor }>(`/api/v1/conversations/${conversationId}/read-cursor`, { method: 'PUT', body: JSON.stringify({ sequence }) })
      const { user_id: who, sequence: position } = result.cursor
      if (activeIdRef.current === conversationId) setReadCursors((current) => ({ ...current, [who]: position }))
    } catch { /* A later message or reconnect retries cursor synchronization. */ }
  }, [])

  const recordDelivered = useCallback((sequence: number) => {
    const conversationId = activeIdRef.current
    if (!conversationId || sequence <= 0) return
    const socket = socketRef.current
    if (socket?.readyState === WebSocket.OPEN) {
      socket.send(JSON.stringify({ type: 'ack', conversation_id: conversationId, sequence }))
      return
    }
    void api<{ cursor: ReadCursor }>(`/api/v1/conversations/${conversationId}/delivery-cursor`, {
      method: 'PUT',
      body: JSON.stringify({ sequence }),
    }).then(({ cursor }) => {
      const { user_id: who, sequence: position } = cursor
      setDeliveryCursors((current) => ({ ...current, [who]: position }))
    }).catch(() => {})
  }, [socketRef])

  /** Reloads recent messages, reactions and cursors after the socket (re)connects. */
  const resync = useCallback(async () => {
    const conversationId = activeIdRef.current
    if (!conversationId) return
    try {
      const synced = await api<{ messages: Message[] }>(`/api/v1/conversations/${conversationId}/messages?limit=100`)
      if (activeIdRef.current === conversationId) mergeMessages(synced.messages)
      const messageIds = synced.messages.map((message) => message.id)
      if (messageIds.length) {
        const reactionPage = await api<{ reactions: MessageReaction[] }>(`/api/v1/conversations/${conversationId}/reactions?message_ids=${messageIds.join(',')}`)
        if (activeIdRef.current === conversationId) replaceReactionsForMessages(messageIds, reactionPage.reactions)
      }
      const [readPage, deliveryPage] = await Promise.all([
        api<{ cursors: ReadCursor[] }>(`/api/v1/conversations/${conversationId}/read-cursors`),
        api<{ cursors: ReadCursor[] }>(`/api/v1/conversations/${conversationId}/delivery-cursors`),
      ])
      if (activeIdRef.current === conversationId) {
        setReadCursors(toCursorMap(readPage.cursors))
        setDeliveryCursors(toCursorMap(deliveryPage.cursors))
      }
      const latestSequence = synced.messages.reduce((latest, message) => Math.max(latest, message.sequence), 0)
      if (latestSequence > 0) recordDelivered(latestSequence)
    } catch { /* A later reconnect can recover this page. */ }
  }, [mergeMessages, replaceReactionsForMessages, recordDelivered])

  /** Applies a reaction event once, ignoring replays of the same event id. */
  const applyReactionEvent = useCallback((eventId: string, messageId: string, emoji: string, added: boolean) => {
    const seen = seenReactionEventIds.current
    if (seen.has(eventId)) return
    seen.add(eventId)
    if (seen.size > 1024) seen.delete(seen.values().next().value!)
    updateReactionCount(messageId, emoji, added)
  }, [updateReactionCount])

  const applyReadCursor = useCallback((userId: string, sequence: number) => setReadCursors((current) => ({ ...current, [userId]: sequence })), [])
  const applyDeliveryCursor = useCallback((userId: string, sequence: number) => setDeliveryCursors((current) => ({ ...current, [userId]: sequence })), [])

  // Load the first page and positions whenever the open conversation changes.
  useEffect(() => {
    if (!activeId) return
    let alive = true
    Promise.all([
      api<{ messages: Message[]; next_before: number | null }>(`/api/v1/conversations/${activeId}/messages?limit=50`),
      api<{ members: User[] }>(`/api/v1/conversations/${activeId}/members`),
      api<{ cursors: ReadCursor[] }>(`/api/v1/conversations/${activeId}/read-cursors`),
      api<{ cursors: ReadCursor[] }>(`/api/v1/conversations/${activeId}/delivery-cursors`),
    ])
      .then(async ([page, memberPage, cursorPage, deliveryPage]) => {
        if (!alive) return
        const chronological = [...page.messages].reverse()
        setMessages(chronological)
        setReactions({})
        setOlder({ conversationId: activeId, cursor: page.next_before })
        setProfiles((current) => ({ ...current, ...Object.fromEntries(memberPage.members.map((member) => [member.id, member])) }))
        setReadCursors(toCursorMap(cursorPage.cursors))
        setDeliveryCursors(toCursorMap(deliveryPage.cursors))
        setLoadedConversationId(activeId)
        if (chronological.length) {
          const messageIds = chronological.map((message) => message.id)
          const reactionPage = await api<{ reactions: MessageReaction[] }>(`/api/v1/conversations/${activeId}/reactions?message_ids=${messageIds.join(',')}`)
          if (!alive) return
          replaceReactionsForMessages(messageIds, reactionPage.reactions)
        }
        const latest = chronological.at(-1)?.sequence ?? 0
        if (latest > 0) {
          recordDelivered(latest)
          void markRead(latest)
        }
      })
      .catch((cause: unknown) => {
        if (!alive) return
        setLoadedConversationId(activeId)
        setMessages([])
        onError(errorMessage(cause, 'Could not load messages'))
      })
    return () => { alive = false }
  }, [activeId, markRead, recordDelivered, replaceReactionsForMessages, onError])

  /** Resolves true when an older page was loaded. */
  const loadOlder = useCallback(async (): Promise<boolean> => {
    if (!activeId || olderCursor === null || loadingOlder) return false
    setLoadingOlder(true)
    try {
      const page = await api<{ messages: Message[]; next_before: number | null }>(`/api/v1/conversations/${activeId}/messages?before_sequence=${olderCursor}&limit=50`)
      setMessages((current) => mergeBySequence(current, page.messages))
      setOlder({ conversationId: activeId, cursor: page.next_before })
      return true
    } catch (cause) {
      onError(errorMessage(cause, 'Could not load older messages'))
      return false
    } finally { setLoadingOlder(false) }
  }, [activeId, olderCursor, loadingOlder, onError])

  /** Sends a message; throws (after reporting the error) so the composer can restore its draft. */
  const send = useCallback(async (content: string) => {
    if (!activeId) return
    onError('')
    try {
      const result = await api<{ message: Message }>(`/api/v1/conversations/${activeId}/messages`, {
        method: 'POST',
        body: JSON.stringify({ client_message_id: uuid7(), reply_to_message_id: replyTo?.id ?? null, content }),
      })
      setReplyTo(null)
      mergeMessages([result.message])
      void markRead(result.message.sequence)
    } catch (cause) {
      onError(errorMessage(cause, 'Message could not be sent'))
      throw cause
    }
  }, [activeId, replyTo, mergeMessages, markRead, onError])

  const edit = useCallback(async (message: Message) => {
    const updated = await dialogs.prompt({ title: 'Edit message', label: 'Message', initialValue: message.content, multiline: true, maxLength: 4000, confirmLabel: 'Save' })
    if (updated === null || !updated.trim()) return
    try {
      const result = await api<{ message: Message }>(`/api/v1/conversations/${activeId}/messages/${message.id}`, { method: 'PATCH', body: JSON.stringify({ content: updated.trim() }) })
      mergeMessages([result.message])
    } catch (cause) { onError(errorMessage(cause, 'Could not edit message')) }
  }, [activeId, dialogs, mergeMessages, onError])

  const remove = useCallback(async (message: Message) => {
    if (!await dialogs.confirm({ title: 'Delete this message?', detail: 'This removes its text for everyone in the conversation.', confirmLabel: 'Delete', danger: true })) return
    try {
      const result = await api<{ message: Message }>(`/api/v1/conversations/${activeId}/messages/${message.id}`, { method: 'DELETE' })
      mergeMessages([result.message])
    } catch (cause) { onError(errorMessage(cause, 'Could not delete message')) }
  }, [activeId, dialogs, mergeMessages, onError])

  const toggleReaction = useCallback(async (message: Message, emoji: string) => {
    const key = `${message.id}:${emoji}`
    if (pendingReactions[key] || !activeId) return
    const existing = reactions[message.id]?.find((reaction) => reaction.emoji === emoji)
    const method = existing?.reacted_by_me ? 'DELETE' : 'PUT'
    setPendingReactions((current) => ({ ...current, [key]: true }))
    try {
      const result = await api<{ reactions: MessageReaction[] }>(`/api/v1/conversations/${activeId}/messages/${message.id}/reactions`, { method, body: JSON.stringify({ emoji }) })
      replaceReactionsForMessages([message.id], result.reactions)
    } catch (cause) {
      onError(errorMessage(cause, 'Could not update reaction'))
    } finally {
      setPendingReactions((current) => { const next = { ...current }; delete next[key]; return next })
    }
  }, [activeId, pendingReactions, reactions, replaceReactionsForMessages, onError])

  const reset = useCallback(() => { setMessages([]); setLoadedConversationId(null) }, [])

  const activeMessages = useMemo(() => loadedConversationId === activeId ? messages : [], [loadedConversationId, activeId, messages])

  return {
    activeMessages,
    loading: activeId !== null && loadedConversationId !== activeId,
    olderCursor, loadingOlder, loadOlder,
    replyTo, setReplyTo,
    profiles,
    reactions, pendingReactions, toggleReaction,
    readCursors, deliveryCursors,
    send, edit, remove,
    mergeMessages, markRead, recordDelivered, resync,
    applyReactionEvent, applyReadCursor, applyDeliveryCursor,
    activeIdRef, reset,
  }
}

export type ConversationState = ReturnType<typeof useConversation>
