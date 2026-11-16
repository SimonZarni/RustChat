import { displayName } from '../../lib/format'
import { initial, shortId } from '../../lib/ids'
import type { Conversation, User } from '../../types'

type Props = {
  conversations: Conversation[]
  activeId: string | null
  userId: string
  profiles: Record<string, User>
  presence: Record<string, boolean>
  onSelect: (conversationId: string) => void
  onStart: () => void
}

export function ConversationList({ conversations, activeId, userId, profiles, presence, onSelect, onStart }: Props) {
  return (
    <div className="conversation-list" aria-label="Conversations">
      {conversations.length === 0 ? (
        <div className="sidebar-empty">
          <span className="empty-glyph">◌</span>
          <strong>No conversations yet</strong>
          <span>Start a direct message or bring a group together.</span>
          <button className="outline-button" onClick={onStart}>Start a conversation</button>
        </div>
      ) : conversations.map((conversation) => {
        const online = conversation.kind === 'direct' && conversation.members.some((member) => member !== userId && presence[member])
        return (
          <button key={conversation.id} className={`conversation-item ${activeId === conversation.id ? 'selected' : ''}`} onClick={() => onSelect(conversation.id)}>
            <span className={`conversation-avatar ${conversation.kind}`}>{conversation.kind === 'group' ? 'G' : initial(shortId(conversation.members.find((member) => member !== userId) ?? conversation.id))}</span>
            <span className="conversation-summary">
              <strong>{displayName(conversation, userId, profiles)}</strong>
              {online && <i className="presence-dot" role="img" aria-label="Online" title="Online" />}
              <span>{conversation.kind === 'group' ? 'Group' : 'Direct message'} <span className="dot-separator">·</span> {new Date(conversation.created_at).toLocaleDateString()}</span>
            </span>
          </button>
        )
      })}
    </div>
  )
}
