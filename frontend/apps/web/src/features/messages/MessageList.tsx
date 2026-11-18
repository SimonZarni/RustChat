import { useLayoutEffect, useRef, useState } from 'react'
import { initial, shortId } from '../../lib/ids'
import { timeLabel } from '../../lib/format'
import { api } from '../../lib/api'
import type { Message, MessageReaction, User } from '../../types'
import { renderMessageContent } from './markdown'
import { MessageActions } from './MessageActions'
import { ReactionChips } from './ReactionControls'

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

/** One quiet word for the newest message you sent; counts only matter once more than one person is involved. */
const deliveryLabel = (deliveredTo: number, readBy: number) =>
  readBy > 0 ? (readBy > 1 ? `Seen by ${readBy}` : 'Seen') : deliveredTo > 0 ? (deliveredTo > 1 ? `Delivered to ${deliveredTo}` : 'Delivered') : 'Sent'

const countReached = (cursors: Record<string, number>, userId: string, sequence: number) =>
  Object.entries(cursors).filter(([reader, position]) => reader !== userId && position >= sequence).length

export function MessageList({
  user, title, messages, loading, olderCursor, loadingOlder, onLoadOlder, profiles, reactions, pendingReactions,
  readCursors, deliveryCursors, typingUserId, onToggleReaction, onReply, onEdit, onDelete,
}: Props) {
  const scrollRef = useRef<HTMLDivElement | null>(null)
  const endRef = useRef<HTMLDivElement | null>(null)
  const heightBeforePrepend = useRef<number | null>(null)
  // The message last touched keeps its action buttons visible (essential on touch screens).
  const [activeId, setActiveId] = useState<string | null>(null)
  const lastOwnId = [...messages].reverse().find((item) => item.sender_id === user.id && !item.deleted_at)?.id

  async function downloadAttachment(attachmentId: string) {
    try {
      const result = await api<{ download_url: string }>(`/api/v1/attachments/${attachmentId}/download`)
      const link = document.createElement('a')
      link.href = result.download_url
      link.rel = 'noopener'
      document.body.append(link)
      link.click()
      link.remove()
    } catch { /* The API error is also surfaced by the next conversation refresh. */ }
  }

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
              <article className={`message-row ${own ? 'own' : ''} ${grouped ? 'grouped' : ''} ${activeId === message.id ? 'active' : ''}`} key={message.id} onClick={() => setActiveId(message.id)}>
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
                  {!message.deleted_at && Boolean(message.attachments?.length) && <ul className="message-attachments" aria-label="Message attachments">
                    {message.attachments?.map((attachment) => <li key={attachment.id}>
                      <span className="attachment-file-mark" aria-hidden="true">↗</span>
                      <span><strong>{attachment.file_name}</strong><small>{attachment.media_type} · {(attachment.byte_size / 1024 / 1024).toFixed(1)} MB</small></span>
                      <button type="button" onClick={() => void downloadAttachment(attachment.id)} aria-label={`Download ${attachment.file_name}`}>Download</button>
                    </li>)}
                  </ul>}
                  {!message.deleted_at && <ReactionChips message={message} reactions={reactions[message.id]} pending={pendingReactions} onToggle={onToggleReaction} />}
                  {(message.edited_at || message.id === lastOwnId) && (
                    <div className="message-status">
                      {message.edited_at && <small>edited</small>}
                      {message.id === lastOwnId && <small>{deliveryLabel(deliveredTo, readBy)}</small>}
                    </div>
                  )}
                  {!message.deleted_at && (
                    <MessageActions
                      message={message} own={own} reactions={reactions[message.id]} pendingReactions={pendingReactions}
                      onToggleReaction={onToggleReaction} onReply={onReply} onEdit={onEdit} onDelete={onDelete}
                    />
                  )}
                </div>
              </article>
            )
          })}
      <div ref={endRef} />
      {typingUserId && <div className="typing-indicator" role="status">{profiles[typingUserId]?.username ?? 'A member'} is typing…</div>}
    </div>
  )
}
