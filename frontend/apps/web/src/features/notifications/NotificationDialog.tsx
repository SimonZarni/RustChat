import type { Notification } from './useNotifications'

export function NotificationDialog({
  items, unreadCount, onClose, onRead, onReadAll, onOpenConversation, onOpenFriends,
}: {
  items: Notification[]; unreadCount: number; onClose: () => void; onRead: (id: string) => Promise<void>
  onReadAll: () => Promise<void>; onOpenConversation: (id: string) => void; onOpenFriends: () => void
}) {
  return <div className="modal-backdrop" onMouseDown={(event) => { if (event.target === event.currentTarget) onClose() }}>
    <section className="dialog notification-dialog" role="dialog" aria-modal="true" aria-labelledby="notifications-title">
      <button className="dialog-close" onClick={onClose} aria-label="Close notifications">×</button>
      <p className="eyebrow">YOUR INBOX</p>
      <div className="notification-heading"><h2 id="notifications-title">Notifications</h2><button className="text-button" disabled={!unreadCount} onClick={() => void onReadAll()}>Mark all read</button></div>
      {items.length === 0 ? <p className="notification-empty">You’re all caught up.</p> : <div className="notification-list">
        {items.map((item) => {
          const actor = item.actor_username ? `@${item.actor_username}` : 'Someone'
          const description = item.kind === 'direct_message' ? 'sent you a message' : item.kind === 'mention' ? 'mentioned you' : 'sent you a friend request'
          return <button className={`notification-row ${item.read_at ? '' : 'unread'}`} key={item.id} onClick={() => {
            void onRead(item.id)
            if (item.conversation_id) { onClose(); onOpenConversation(item.conversation_id) }
            else { onClose(); onOpenFriends() }
          }}>
            <span className="notification-dot" aria-hidden="true" />
            <span className="notification-copy"><strong>{actor}</strong> {description}<time>{new Date(item.created_at).toLocaleString()}</time></span>
          </button>
        })}
      </div>}
    </section>
  </div>
}
