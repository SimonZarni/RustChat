import { useCallback, useEffect, useRef, useState } from 'react'
import { DialogProvider } from './components/Dialogs'
import { AccountFooter } from './features/account/AccountFooter'
import { AuthScreen } from './features/auth/AuthScreen'
import { VerifyEmailScreen } from './features/auth/VerifyEmailScreen'
import { CommunitySection } from './features/communities/CommunitySection'
import { useCommunities } from './features/communities/useCommunities'
import { ConversationList } from './features/conversations/ConversationList'
import { NewConversationDialog } from './features/conversations/NewConversationDialog'
import { ChatPanel } from './features/messages/ChatPanel'
import { useConversation } from './features/messages/useConversation'
import { useRealtime } from './features/realtime/useRealtime'
import { api, errorMessage } from './lib/api'
import { conversationShortcut, displayName } from './lib/format'
import { readVerifyEmailToken } from './lib/links'
import { useTheme } from './lib/useTheme'
import type { Conversation, User } from './types'

export default function App() {
  return <DialogProvider><Shell /></DialogProvider>
}

/** Application shell: session lifecycle and the layout that wires the feature modules together. */
function Shell() {
  const [theme, setTheme] = useTheme()
  const [user, setUser] = useState<User | null>(null)
  const [checkingSession, setCheckingSession] = useState(true)
  const [signedOutNotice, setSignedOutNotice] = useState('')
  const [error, setError] = useState('')
  const [verifyEmailToken, setVerifyEmailToken] = useState(readVerifyEmailToken)
  const [conversations, setConversations] = useState<Conversation[]>([])
  const [activeId, setActiveId] = useState<string | null>(null)
  const [showNewConversation, setShowNewConversation] = useState(false)
  const [mobileList, setMobileList] = useState(true)
  const socketRef = useRef<WebSocket | null>(null)

  const communities = useCommunities(user, setError)
  const conversation = useConversation(activeId, socketRef, setError)
  const realtime = useRealtime({ user, activeId, socketRef, conversation, communities })

  const openConversation = useCallback((conversationId: string) => {
    setActiveId(conversationId)
    setMobileList(false)
  }, [])

  useEffect(() => {
    let alive = true
    api<{ user: User }>('/api/v1/auth/me')
      .then(({ user: current }) => { if (alive) setUser(current) })
      .catch(() => {})
      .finally(() => { if (alive) setCheckingSession(false) })
    return () => { alive = false }
  }, [])

  const userId = user?.id
  useEffect(() => {
    if (!userId) return
    api<{ conversations: Conversation[] }>('/api/v1/conversations?limit=100')
      .then((result) => {
        setConversations(result.conversations)
        setActiveId((current) => current && result.conversations.some((item) => item.id === current) ? current : result.conversations[0]?.id ?? null)
      })
      .catch((cause: unknown) => setError(errorMessage(cause, 'Could not load conversations')))
  }, [userId])

  // Ctrl/⌘ + K opens the new-conversation dialog unless the user is typing.
  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      const target = event.target
      const typing = target instanceof HTMLElement && (target.isContentEditable || ['INPUT', 'TEXTAREA', 'SELECT'].includes(target.tagName))
      if (!typing && (event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'k') {
        event.preventDefault()
        setShowNewConversation(true)
      }
    }
    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  }, [])

  function endSession(notice: string) {
    setUser(null); setError(''); setSignedOutNotice(notice)
    setConversations([]); setActiveId(null)
    conversation.reset()
    communities.reset()
  }

  async function signOut() {
    try { await api('/api/v1/auth/logout', { method: 'POST' }) } catch { /* Clear local state even if the session expired. */ }
    endSession('')
  }

  if (verifyEmailToken !== null) {
    return <VerifyEmailScreen token={verifyEmailToken} onContinue={() => { window.history.replaceState({}, '', '/'); setVerifyEmailToken(null) }} />
  }
  if (checkingSession) return <main className="loading-screen"><span className="brand-mark">R</span><p>Opening RustChat…</p></main>
  if (!user) return <AuthScreen onAuthenticated={setUser} initialNotice={signedOutNotice} />

  const activeConversation = conversations.find((item) => item.id === activeId)
  const activeChannel = communities.channels.find((channel) => channel.conversation_id === activeId)
  const selectedCommunity = communities.communities.find((item) => item.id === communities.selectedCommunityId)
  const selectedChannelCount = communities.channels.filter((channel) => channel.community_id === communities.selectedCommunityId).length
  const directPeerName = activeConversation?.kind === 'direct'
    ? activeConversation.members.filter((memberId) => memberId !== user.id).map((memberId) => conversation.profiles[memberId]?.username).find(Boolean)
    : undefined
  const title = activeChannel?.name ?? (activeConversation ? directPeerName ?? displayName(activeConversation, user.id) : 'Your messages')
  const active = activeId && (activeConversation || activeChannel) ? {
    id: activeId,
    title,
    subtitle: activeConversation?.kind === 'group'
      ? `${activeConversation.members.length} members`
      : activeChannel ? `${selectedChannelCount} channels · ${selectedCommunity?.name ?? 'Community'}` : 'Private conversation',
    avatar: activeChannel ? '#' : activeConversation?.kind === 'group' ? 'G' : 'D',
    placeholder: activeChannel ? `Message #${activeChannel.name}` : `Message ${title}`,
    readOnly: activeChannel?.read_only ?? false,
  } : null

  return (
    <main className="workspace">
      <aside className="workspace-rail">
        <div className="rail-brand" aria-label="RustChat">R</div>
        <div className="rail-divider" />
        <div className="rail-avatar" title={user.username}>{user.username.slice(0, 1).toUpperCase()}</div>
        <div className="rail-spacer" />
        <button className="rail-account" onClick={() => void signOut()} aria-label={`Sign out ${user.username}`} title="Sign out">↗</button>
      </aside>
      <aside className={`conversation-sidebar ${mobileList ? 'mobile-visible' : ''}`}>
        <header className="sidebar-header">
          <div><p className="eyebrow">YOUR WORKSPACE</p><h1>Messages</h1></div>
          <button className="new-button" onClick={() => setShowNewConversation(true)} aria-label="New conversation" title="New conversation">+</button>
        </header>
        <div className="sidebar-search"><span aria-hidden="true">⌕</span><span>Conversations</span><kbd>{conversationShortcut}</kbd></div>
        <CommunitySection
          state={communities} user={user} activeId={activeId} onOpenConversation={openConversation}
          onClearActive={() => setActiveId(null)} onChannelDeleted={(id) => setActiveId((current) => current === id ? null : current)} onError={setError}
        />
        <div className="section-heading dm-heading">
          <span>DIRECT MESSAGES</span>
          <button onClick={() => setShowNewConversation(true)} aria-label="New direct message">+</button>
        </div>
        <ConversationList
          conversations={conversations} activeId={activeId} userId={user.id} profiles={conversation.profiles} presence={realtime.presence}
          onSelect={(id) => { openConversation(id); setError('') }} onStart={() => setShowNewConversation(true)}
        />
        <AccountFooter
          user={user} theme={theme} onThemeChange={setTheme} onUserChange={setUser} onSignOut={() => void signOut()}
          onAccountClosed={endSession} onError={setError}
        />
      </aside>
      <ChatPanel
        user={user} active={active} hiddenOnMobile={mobileList} connected={realtime.connected} error={error}
        onDismissError={() => setError('')} onBack={() => setMobileList(true)} onStart={() => setShowNewConversation(true)}
        conversation={conversation} typingUserId={realtime.typingUserId} onTyping={realtime.sendTyping}
      />
      {showNewConversation && (
        <NewConversationDialog
          onClose={() => setShowNewConversation(false)} onError={setError}
          onCreated={(created) => { setConversations((current) => [created, ...current.filter((item) => item.id !== created.id)]); openConversation(created.id) }}
        />
      )}
    </main>
  )
}
