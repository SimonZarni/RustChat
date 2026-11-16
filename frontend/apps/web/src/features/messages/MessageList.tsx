import { useLayoutEffect, useRef } from 'react'
import { initial, shortId } from '../../lib/ids'
import { timeLabel } from '../../lib/format'
import { REACTION_EMOJI } from '../../lib/permissions'
import type { Message, MessageReaction, User } from '../../types'
import { renderMessageContent } from './markdown'

const GROUPING_WINDOW_MS = 5 * 60 * 1000

type Props = {
  user: User
  title: string
  messages: Message[]
  loading: boolean
  olderCursor: number | null
  loadingOlder: boolean
  onLoadOlder: () => Promise<boolean>
  profiles: Record<string, User>
  reactions: Record<string, MessageReaction[]>
  pendingReactions: Record<string, boolean>
  readCursors: Record<string, number>
  deliveryCursors: Record<string, number>
  typingUserId: string | null
  onToggleReaction: (message: Message, emoji: string) => void
  onReply: (message: Message) => void
  onEdit: (message: Message) => void
  onDelete: (message: Message) => void
}

const countReached = (cursors: Record<string, number>, userId: string, sequence: number) =>
  Object.entries(cursors).filter(([reader, position]) => reader !== userId && position >= sequence).length

export function MessageList({
  user, title, messages, loading, olderCursor, loadingOlder, onLoadOlder, profiles, reactions, pendingReactions,
  readCursors, deliveryCursors, typingUserId, onToggleReaction, onReply, onEdit, onDelete,
}: Props) {
  const scrollRef = useRef<HTMLDivElement | null>(null)
  const endRef = useRef<HTMLDivElement | null>(null)
  const heightBeforePrepend = useRef<number | null>(null)

  // Keep the viewport anchored when older messages are prepended; otherwise follow new messages.
  useLayoutEffect(() => {
    const container = scrollRef.current
    if (container && heightBeforePrepend.current !== null) {
      container.scrollTop += container.scrollHeight - heightBeforePrepend.current
      heightBeforePrepend.current = null
    } else {
      endRef.current?.scrollIntoView?.({ behavior: 'smooth', block: 'end' })
    }
  }, [messages])

  async function loadOlder() {
    heightBeforePrepend.current = scrollRef.current?.scrollHeight ?? null
    if (!await onLoadOlder()) heightBeforePrepend.current = null
  }

  return (
    <div className="message-scroll" ref={scrollRef} role="log" aria-label={`Messages in ${title}`} aria-live="polite" aria-relevant="additions" aria-busy={loading}>
      {olderCursor !== null && <button className="load-older-button" onClick={() => void loadOlder()} disabled={loadingOlder}>{loadingOlder ? 'Loading older messages…' : 'Load older messages'}</button>}
      {olderCursor === null && <div className="history-start"><span className="history-line" /><span>THIS IS THE BEGINNING</span><span className="history-line" /></div>}
      {loading
        ? <div className="message-placeholder">Loading messages…</div>
        : messages.length === 0
          ? <div className="message-placeholder"><span className="empty-glyph">◌</span><strong>A fresh conversation</strong><span>Send a message to get things started.</span></div>
          : messages.map((message, index) => {
            const own = message.sender_id === user.id
            const prior = messages[index - 1]
            const grouped = prior?.sender_id === message.sender_id && new Date(message.created_at).getTime() - new Date(prior.created_at).getTime() < GROUPING_WINDOW_MS
            const deliveredTo = countReached(deliveryCursors, user.id, message.sequence)
            const readBy = countReached(readCursors, user.id, message.sequence)
            const quoted = message.reply_to_message_id ? messages.find((item) => item.id === message.reply_to_message_id) : undefined
            return (
              <article className={`message-row ${own ? 'own' : ''} ${grouped ? 'grouped' : ''}`} key={message.id}>
                <div className="message-avatar">{own ? initial(user.username) : initial(profiles[message.sender_id]?.username ?? '?')}</div>
                <div className="message-content">
                  {!grouped && (
                    <div className="message-meta">
                      <strong>{own ? user.username : (profiles[message.sender_id]?.username ?? `Member ${shortId(message.sender_id)}`)}</strong>
                      <time>{timeLabel(message.created_at)}</time>
                    </div>
                  )}
                  {message.reply_to_message_id && <blockquote className="reply-quote">{quoted?.content ?? 'Original message'}</blockquote>}
                  <div className="message-body">{renderMessageContent(message, profiles)}</div>
                  {!message.deleted_at && (
                    <div className="message-reactions" aria-label="Reactions">
                      {REACTION_EMOJI.map((emoji) => {
                        const reaction = reactions[message.id]?.find((item) => item.emoji === emoji)
                        return (
                          <button
                            key={emoji} type="button" className={reaction?.reacted_by_me ? 'selected' : ''}
                            aria-pressed={reaction?.reacted_by_me ?? false}
                            aria-label={`${reaction?.reacted_by_me ? 'Remove' : 'Add'} ${emoji} reaction${reaction ? `, ${reaction.count} total` : ''}`}
                            disabled={pendingReactions[`${message.id}:${emoji}`]}
                            onClick={() => onToggleReaction(message, emoji)}
                          >{emoji}{reaction && <span>{reaction.count}</span>}</button>
                        )
                      })}
                    </div>
                  )}
                  <div className="message-footer">
                    <button className="message-link" type="button" onClick={() => onReply(message)}>Reply</button>
                    {!message.deleted_at && own && <button className="message-link" type="button" onClick={() => onEdit(message)}>Edit</button>}
                    {!message.deleted_at && own && <button className="message-link danger" type="button" onClick={() => onDelete(message)}>Delete</button>}
                    {own && deliveredTo > 0 && <small>Delivered to {deliveredTo}</small>}
                    {own && deliveredTo === 0 && <small>Sent</small>}
                    {own && readBy > 0 && <small>Seen by {readBy}</small>}
                    {message.edited_at && <small>edited</small>}
                  </div>
                </div>
              </article>
            )
          })}
      <div ref={endRef} />
      {typingUserId && <div className="typing-indicator" role="status">{profiles[typingUserId]?.username ?? 'A member'} is typing…</div>}
    </div>
  )
}
