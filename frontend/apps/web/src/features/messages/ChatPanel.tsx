import type { User } from '../../types'
import { MessageComposer } from './MessageComposer'
import { MessageList } from './MessageList'
import type { ConversationState } from './useConversation'

type Props = {
  user: User
  /** Null when nothing is selected. */
  active: { id: string; title: string; subtitle: string; avatar: string; placeholder: string; readOnly: boolean; voiceCapable: boolean } | null
  hiddenOnMobile: boolean
  connected: boolean
  error: string
  onDismissError: () => void
  onBack: () => void
  onStart: () => void
  onStartVoice: () => void
  conversation: ConversationState
  typingUserId: string | null
  onTyping: (active: boolean) => void
}

export function ChatPanel({ user, active, hiddenOnMobile, connected, error, onDismissError, onBack, onStart, onStartVoice, conversation, typingUserId, onTyping }: Props) {
  return (
    <section className={`chat-panel ${hiddenOnMobile ? 'mobile-hidden' : ''}`}>
      <header className="chat-header">
        {active ? (
          <>
            <button className="mobile-back" onClick={onBack} aria-label="Back to conversations">←</button>
            <div className="chat-title-avatar">{active.avatar}</div>
            <div className="chat-title"><h2>{active.title}</h2><p>{active.subtitle}</p></div>
            <div className="connection-status" role="status" aria-live="polite">
              <span className={connected ? 'status-dot online' : 'status-dot'} />{connected ? 'Live' : 'Reconnecting'}
            </div>
            {active.voiceCapable && <button className="voice-start-button" onClick={onStartVoice} aria-label="Start voice call" title="Start voice call">◖ Voice</button>}
            <button className="more-button" aria-label="Conversation details">···</button>
          </>
        ) : (
          <div className="chat-title"><h2>Your messages</h2><p>A space for thoughtful conversations</p></div>
        )}
      </header>
      {error && <div className="inline-error" role="alert">{error}<button onClick={onDismissError} aria-label="Dismiss">×</button></div>}
      {active ? (
        <>
          <MessageList
            user={user} title={active.title} messages={conversation.activeMessages} loading={conversation.loading}
            olderCursor={conversation.olderCursor} loadingOlder={conversation.loadingOlder} onLoadOlder={conversation.loadOlder}
            profiles={conversation.profiles} reactions={conversation.reactions} pendingReactions={conversation.pendingReactions}
            readCursors={conversation.readCursors} deliveryCursors={conversation.deliveryCursors} typingUserId={typingUserId}
            onToggleReaction={(message, emoji) => void conversation.toggleReaction(message, emoji)}
            onReply={conversation.setReplyTo} onEdit={(message) => void conversation.edit(message)} onDelete={(message) => void conversation.remove(message)}
          />
          <MessageComposer
            key={`${user.id}:${active.id}`} userId={user.id} conversationId={active.id}
            placeholder={active.placeholder} readOnly={active.readOnly}
            replyTo={conversation.replyTo} replyAuthor={conversation.replyTo ? conversation.profiles[conversation.replyTo.sender_id]?.username ?? 'member' : ''}
            onCancelReply={() => conversation.setReplyTo(null)} onSend={conversation.send} onTyping={onTyping}
          />
        </>
      ) : (
        <div className="welcome-panel">
          <span className="welcome-mark">R</span>
          <p className="eyebrow">A GOOD PLACE TO BEGIN</p>
          <h2>Your conversations,<br />all in one place.</h2>
          <p>Choose a conversation from the left, or start a new one to say hello.</p>
          <button className="primary-button compact" onClick={onStart}>Start a conversation <span>↗</span></button>
        </div>
      )}
      <footer className="chat-footer"><span>RUSTCHAT</span><span>Messages are private to their members</span></footer>
    </section>
  )
}
