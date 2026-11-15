import { Fragment, useCallback, useEffect, useLayoutEffect, useRef, useState, type FormEvent, type ReactNode } from 'react'

type User = { id: string; username: string; created_at: string }
type UserProfile = { id: string; username: string; display_name: string; bio: string; updated_at: string }
type Session = { id: string; created_at: string; expires_at: string; current: boolean }
type ProfilePrivacy = { discoverable: boolean; profile_visibility: 'everyone' | 'conversation_members'; presence_visible: boolean }
type Conversation = { id: string; kind: 'direct' | 'group'; created_by: string; created_at: string; members: string[] }
type Message = { id: string; conversation_id: string; sender_id: string; sequence: number; client_message_id: string; reply_to_message_id?: string | null; mentioned_user_ids?: string[]; content: string; created_at: string; edited_at?: string | null; deleted_at?: string | null }
type MessageReaction = { message_id: string; emoji: string; count: number; reacted_by_me: boolean }
type Community = { id: string; owner_id: string; name: string; description: string; created_at: string; member_count: number }
type Channel = { id: string; community_id: string; conversation_id: string; category_id: string | null; name: string; description: string; position: number; read_only: boolean; created_at: string }
type Category = { id: string; community_id: string; name: string; position: number }
type CommunityRole = { id: string; community_id: string; name: string; position: number; permissions: number; is_default: boolean }
type CommunityInvite = { id: string; created_at: string; expires_at: string | null; max_uses: number | null; use_count: number; revoked_at: string | null }
type InstanceUser = { id: string; username: string; created_at: string; is_instance_admin: boolean; deactivated_at: string | null; suspended_at: string | null; deleted_at: string | null }
type ApiError = { error?: { message?: string; code?: string } }

const ROLE_PERMISSIONS = [
  ['View community', 1], ['Manage community', 2], ['Manage channels', 4], ['Manage roles', 8],
  ['Manage members', 16], ['Create invites', 32], ['Send messages', 64], ['Read messages', 128], ['Manage messages', 256],
] as const
const conversationShortcut = typeof navigator !== 'undefined' && /Mac|iPhone|iPad/.test(navigator.platform) ? '⌘ K' : 'Ctrl K'

async function api<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(path, {
    ...init,
    credentials: 'include',
    headers: { ...(init?.body ? { 'Content-Type': 'application/json' } : {}), ...init?.headers },
  })
  if (response.status === 204) return undefined as T
  const data = await response.json().catch(() => ({})) as T & ApiError
  if (!response.ok) throw new Error(data.error?.message ?? data.error?.code ?? `Request failed (${response.status})`)
  return data
}

function uuid7() {
  const bytes = crypto.getRandomValues(new Uint8Array(16))
  let millis = BigInt(Date.now())
  for (let i = 5; i >= 0; i--) { bytes[i] = Number(millis & 255n); millis >>= 8n }
  bytes[6] = ((bytes[6] ?? 0) & 0x0f) | 0x70
  bytes[8] = ((bytes[8] ?? 0) & 0x3f) | 0x80
  const hex = [...bytes].map((byte) => byte.toString(16).padStart(2, '0')).join('')
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`
}

function shortId(id: string) { return id.slice(0, 8) }
function displayName(conversation: Conversation, currentUserId: string, profiles: Record<string, User> = {}) {
  if (conversation.kind === 'group') return `Group conversation · ${conversation.members.length}`
  const peer = conversation.members.find((member) => member !== currentUserId)
  return peer ? profiles[peer]?.username ?? `Direct · ${shortId(peer)}` : 'Direct conversation'
}
function timeLabel(value: string) {
  return new Intl.DateTimeFormat(undefined, { hour: 'numeric', minute: '2-digit' }).format(new Date(value))
}

function renderMessageContent(message: Message, profiles: Record<string, User>) {
  const mentionedNames = new Set((message.mentioned_user_ids ?? [])
    .map((id) => profiles[id]?.username.toLowerCase())
    .filter((name): name is string => Boolean(name)))
  const inline = (value: string, keyPrefix: string): ReactNode[] => {
    const parts = value.split(/(`[^`]+`|\*\*[^*]+\*\*|~~[^~]+~~|\*[^*]+\*|\[[^\]]+\]\(https?:\/\/[^\s)]+\)|https?:\/\/[^\s<>()]+|@[A-Za-z0-9_]{3,32})/g)
    return parts.map((part, index) => {
      const key = `${keyPrefix}-${index}`
      if (part.startsWith('`') && part.endsWith('`')) return <code key={key}>{part.slice(1, -1)}</code>
      if (part.startsWith('**') && part.endsWith('**')) return <strong key={key}>{part.slice(2, -2)}</strong>
      if (part.startsWith('~~') && part.endsWith('~~')) return <del key={key}>{part.slice(2, -2)}</del>
      if (part.startsWith('*') && part.endsWith('*')) return <em key={key}>{part.slice(1, -1)}</em>
      const markdownLink = part.match(/^\[([^\]]+)\]\((https?:\/\/[^\s)]+)\)$/)
      if (markdownLink) return <a key={key} href={markdownLink[2]} target="_blank" rel="noopener noreferrer">{markdownLink[1]}</a>
      if (/^https?:\/\//i.test(part)) {
        const suffix = part.match(/[.,!?;:]+$/)?.[0] ?? ''
        const url = suffix ? part.slice(0, -suffix.length) : part
        return <Fragment key={key}><a href={url} target="_blank" rel="noopener noreferrer">{url}</a>{suffix}</Fragment>
      }
      if (part.startsWith('@') && mentionedNames.has(part.slice(1).toLowerCase())) return <span className="mention-highlight" key={key}>{part}</span>
      return part
    })
  }

  const blocks: ReactNode[] = []
  const paragraphs: string[] = []
  let listItems: string[] = []
  let orderedList = false
  let codeLines: string[] = []
  let inCode = false
  const flushParagraph = () => {
    if (paragraphs.length) blocks.push(<p key={`p-${blocks.length}`}>{paragraphs.map((line, index) => <Fragment key={index}>{index > 0 && <br />}{inline(line, `p${blocks.length}-${index}`)}</Fragment>)}</p>)
    paragraphs.length = 0
  }
  const flushList = () => {
    if (listItems.length) {
      const List = orderedList ? 'ol' : 'ul'
      blocks.push(<List key={`list-${blocks.length}`}>{listItems.map((line, index) => <li key={index}>{inline(line, `list${blocks.length}-${index}`)}</li>)}</List>)
    }
    listItems = []
  }
  const lines = message.content.split('\n')
  for (const [index, line] of lines.entries()) {
    if (line.trimStart().startsWith('```')) {
      flushParagraph(); flushList()
      if (inCode) {
        blocks.push(<pre key={`code-${blocks.length}`}><code>{codeLines.join('\n')}</code></pre>)
        codeLines = []
      }
      inCode = !inCode
      continue
    }
    if (inCode) { codeLines.push(line); continue }
    const item = line.match(/^\s*([-*+]|\d+\.)\s+(.+)$/)
    if (item) {
      flushParagraph()
      const nextOrdered = /^\d/.test(item[1] ?? '')
      if (listItems.length && nextOrdered !== orderedList) flushList()
      orderedList = nextOrdered
      listItems.push(item[2] ?? '')
    } else if (!line.trim()) {
      flushParagraph(); flushList()
    } else {
      flushList(); paragraphs.push(line)
    }
    if (index === lines.length - 1) { flushParagraph(); flushList() }
  }
  if (inCode) blocks.push(<pre key={`code-${blocks.length}`}><code>{codeLines.join('\n')}</code></pre>)
  flushParagraph(); flushList()
  return <>{blocks}</>
}

export default function App() {
  const [theme, setTheme] = useState<'light' | 'dark'>(() => localStorage.getItem('rustchat-theme') === 'dark' ? 'dark' : 'light')
  const [user, setUser] = useState<User | null>(null)
  const [profile, setProfile] = useState<UserProfile | null>(null)
  const [profileBusy, setProfileBusy] = useState(false)
  const [sessions, setSessions] = useState<Session[]>([])
  const [currentPassword, setCurrentPassword] = useState('')
  const [newPassword, setNewPassword] = useState('')
  const [passwordBusy, setPasswordBusy] = useState(false)
  const [securityMessage, setSecurityMessage] = useState('')
  const [accountPassword, setAccountPassword] = useState('')
  const [deleteConfirmation, setDeleteConfirmation] = useState('')
  const [accountActionBusy, setAccountActionBusy] = useState(false)
  const [privacy, setPrivacy] = useState<ProfilePrivacy | null>(null)
  const [privacyBusy, setPrivacyBusy] = useState(false)
  const [instanceUsers, setInstanceUsers] = useState<InstanceUser[] | null>(null)
  const [checkingSession, setCheckingSession] = useState(true)
  const [authMode, setAuthMode] = useState<'login' | 'register'>('login')
  const [authEmail, setAuthEmail] = useState('')
  const [recoveryToken] = useState(() => {
    const path = window.location.pathname
    return path.endsWith('/reset-password') ? new URLSearchParams(window.location.search).get('token') ?? '' : ''
  })
  const [recoveryMode, setRecoveryMode] = useState<'request' | 'reset' | null>(() => recoveryToken ? 'reset' : null)
  const [recoveryPassword, setRecoveryPassword] = useState('')
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  const [authMessage, setAuthMessage] = useState('')
  const [busy, setBusy] = useState(false)
  const [conversations, setConversations] = useState<Conversation[]>([])
  const [communities, setCommunities] = useState<Community[]>([])
  const [selectedCommunityId, setSelectedCommunityId] = useState<string | null>(null)
  const [channels, setChannels] = useState<Channel[]>([])
  const [categories, setCategories] = useState<Category[]>([])
  const [newCategoryName, setNewCategoryName] = useState('')
  const [channelCategoryId, setChannelCategoryId] = useState('')
  const [activeId, setActiveId] = useState<string | null>(null)
  const [messages, setMessages] = useState<Message[]>([])
  const [replyToMessage, setReplyToMessage] = useState<Message | null>(null)
  const [loadedConversationId, setLoadedConversationId] = useState<string | null>(null)
  const [profiles, setProfiles] = useState<Record<string, User>>({})
  const [olderCursor, setOlderCursor] = useState<number | null>(null)
  const [loadingOlder, setLoadingOlder] = useState(false)
  const [draft, setDraft] = useState('')
  const [error, setError] = useState('')
  const [showComposer, setShowComposer] = useState(false)
  const [showCommunityComposer, setShowCommunityComposer] = useState(false)
  const [showJoinComposer, setShowJoinComposer] = useState(false)
  const [showChannelComposer, setShowChannelComposer] = useState(false)
  const [showCommunityManager, setShowCommunityManager] = useState(false)
  const [communityRoles, setCommunityRoles] = useState<CommunityRole[]>([])
  const [communityMembers, setCommunityMembers] = useState<User[]>([])
  const [communityInvites, setCommunityInvites] = useState<CommunityInvite[]>([])
  const [newRoleName, setNewRoleName] = useState('')
  const [newRolePermissions, setNewRolePermissions] = useState(193)
  const [selectedRoleId, setSelectedRoleId] = useState('')
  const [selectedMemberId, setSelectedMemberId] = useState('')
  const [communityEditName, setCommunityEditName] = useState('')
  const [communityEditDescription, setCommunityEditDescription] = useState('')
  const [overrideChannelId, setOverrideChannelId] = useState('')
  const [overrideAllow, setOverrideAllow] = useState(0)
  const [overrideDeny, setOverrideDeny] = useState(0)
  const [communityName, setCommunityName] = useState('')
  const [inviteCode, setInviteCode] = useState('')
  const [createdInviteCode, setCreatedInviteCode] = useState('')
  const [inviteCopied, setInviteCopied] = useState(false)
  const [channelName, setChannelName] = useState('')
  const [conversationKind, setConversationKind] = useState<'direct' | 'group'>('direct')
  const [participants, setParticipants] = useState('')
  const [profilePreview, setProfilePreview] = useState<UserProfile | null>(null)
  const [profilePreviewError, setProfilePreviewError] = useState('')
  const [profilePreviewBusy, setProfilePreviewBusy] = useState(false)
  const [creating, setCreating] = useState(false)
  const [connected, setConnected] = useState(false)
  const [typingUserId, setTypingUserId] = useState<string | null>(null)
  const [readCursors, setReadCursors] = useState<Record<string, number>>({})
  const [deliveryCursors, setDeliveryCursors] = useState<Record<string, number>>({})
  const [presence, setPresence] = useState<Record<string, boolean>>({})
  const [reactions, setReactions] = useState<Record<string, MessageReaction[]>>({})
  const [pendingReactions, setPendingReactions] = useState<Record<string, boolean>>({})
  const [mobileList, setMobileList] = useState(true)
  const modalOpen = showComposer || showCommunityComposer || showJoinComposer || showChannelComposer || showCommunityManager
  const modalReturnFocusRef = useRef<HTMLElement | null>(null)
  const openModal = (setOpen: (open: boolean) => void) => {
    modalReturnFocusRef.current = document.activeElement instanceof HTMLElement ? document.activeElement : null
    setOpen(true)
  }

  useEffect(() => {
    document.documentElement.dataset.theme = theme
    localStorage.setItem('rustchat-theme', theme)
  }, [theme])
  const socketRef = useRef<WebSocket | null>(null)
  const selectedCommunityIdRef = useRef<string | null>(selectedCommunityId)
  const showCommunityManagerRef = useRef(showCommunityManager)
  selectedCommunityIdRef.current = selectedCommunityId
  showCommunityManagerRef.current = showCommunityManager
  const activeIdRef = useRef<string | null>(null)
  const lastSequenceRef = useRef(0)
  const seenReactionEventIdsRef = useRef(new Set<string>())
  const messagesEndRef = useRef<HTMLDivElement | null>(null)
  const messageScrollRef = useRef<HTMLDivElement | null>(null)
  const scrollHeightBeforePrependRef = useRef<number | null>(null)
  const typingExpiryRef = useRef<number | null>(null)

  useEffect(() => {
    function handleGlobalKeyDown(event: KeyboardEvent) {
      const target = event.target
      const typing = target instanceof HTMLElement && (target.isContentEditable || ['INPUT', 'TEXTAREA', 'SELECT'].includes(target.tagName))
      if (!typing && (event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'k') {
        event.preventDefault()
        modalReturnFocusRef.current = document.activeElement instanceof HTMLElement ? document.activeElement : null
        openModal(setShowComposer)
      } else if (event.key === 'Escape') {
        setShowComposer(false)
        setShowCommunityComposer(false)
        setShowJoinComposer(false)
        setShowChannelComposer(false)
        setShowCommunityManager(false)
      }
    }
    window.addEventListener('keydown', handleGlobalKeyDown)
    return () => window.removeEventListener('keydown', handleGlobalKeyDown)
  }, [])

  useLayoutEffect(() => {
    if (!modalOpen) return
    const dialog = document.querySelector<HTMLElement>('[role="dialog"][aria-modal="true"]')
    if (!dialog) return
    const dialogElement = dialog
    const previousFocus = modalReturnFocusRef.current ?? document.activeElement
    modalReturnFocusRef.current = null
    const getFocusable = () => Array.from(dialogElement.querySelectorAll<HTMLElement>(
      'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])',
    )).filter((element) => element.getAttribute('aria-hidden') !== 'true')
    const focusable = getFocusable()
    const initialFocus = dialogElement.querySelector<HTMLElement>('[autofocus]')
      ?? dialogElement.querySelector<HTMLElement>('input:not([disabled]), textarea:not([disabled]), select:not([disabled])')
      ?? focusable[0]
    initialFocus?.focus()
    function keepFocusInside(event: KeyboardEvent) {
      if (event.key !== 'Tab') return
      const elements = getFocusable()
      const first = elements[0]
      const last = elements.at(-1)
      if (!first || !last) { event.preventDefault(); return }
      if (event.shiftKey && (document.activeElement === first || !dialogElement.contains(document.activeElement))) {
        event.preventDefault(); last.focus()
      } else if (!event.shiftKey && (document.activeElement === last || !dialogElement.contains(document.activeElement))) {
        event.preventDefault(); first.focus()
      }
    }
    document.addEventListener('keydown', keepFocusInside)
    return () => {
      document.removeEventListener('keydown', keepFocusInside)
      if (previousFocus instanceof HTMLElement && previousFocus.isConnected) previousFocus.focus()
    }
  }, [modalOpen])

  const markRead = useCallback(async (sequence: number) => {
    const conversationId = activeIdRef.current
    if (!conversationId || sequence <= 0) return
    try {
      const result = await api<{ cursor: { user_id: string; sequence: number } }>(`/api/v1/conversations/${conversationId}/read-cursor`, { method: 'PUT', body: JSON.stringify({ sequence }) })
      if (activeIdRef.current === conversationId) setReadCursors((current) => ({ ...current, [result.cursor.user_id]: result.cursor.sequence }))
    } catch { /* A later message or reconnect retries cursor synchronization. */ }
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

  const recordDelivered = useCallback((sequence: number) => {
    const conversationId = activeIdRef.current
    if (!conversationId || sequence <= 0) return
    const socket = socketRef.current
    if (socket?.readyState === WebSocket.OPEN) {
      socket.send(JSON.stringify({ type: 'ack', conversation_id: conversationId, sequence }))
      return
    }
    void api<{ cursor: { user_id: string; sequence: number } }>(`/api/v1/conversations/${conversationId}/delivery-cursor`, {
      method: 'PUT',
      body: JSON.stringify({ sequence }),
    }).then(({ cursor }) => setDeliveryCursors((current) => ({ ...current, [cursor.user_id]: cursor.sequence }))).catch(() => {})
  }, [])

  const loadConversations = useCallback(async () => {
    const result = await api<{ conversations: Conversation[] }>('/api/v1/conversations?limit=100')
    setConversations(result.conversations)
    setActiveId((current) => current && result.conversations.some((item) => item.id === current) ? current : result.conversations[0]?.id ?? null)
  }, [])

  const loadCommunities = useCallback(async () => {
    const result = await api<{ communities: Community[] }>('/api/v1/communities')
    setCommunities(result.communities)
    setSelectedCommunityId((current) => current && result.communities.some((item) => item.id === current) ? current : result.communities[0]?.id ?? null)
  }, [])

  useEffect(() => {
    let alive = true
    api<{ user: User }>('/api/v1/auth/me').then(({ user: currentUser }) => {
      if (alive) setUser(currentUser)
    }).catch(() => {}).finally(() => { if (alive) setCheckingSession(false) })
    return () => { alive = false }
  }, [])

  useEffect(() => {
    if (user) {
      api<{ profile: UserProfile }>('/api/v1/auth/profile').then(({ profile: current }) => setProfile(current)).catch((cause: unknown) => setError(cause instanceof Error ? cause.message : 'Could not load profile'))
      loadConversations().catch((cause: unknown) => setError(cause instanceof Error ? cause.message : 'Could not load conversations'))
      loadCommunities().catch((cause: unknown) => setError(cause instanceof Error ? cause.message : 'Could not load communities'))
      api<{ users: InstanceUser[] }>('/api/v1/admin/users').then(({ users: items }) => setInstanceUsers(items)).catch(() => setInstanceUsers(null))
    }
  }, [user, loadCommunities, loadConversations])

  useEffect(() => {
    if (!selectedCommunityId) return
    let alive = true
    Promise.all([
      api<{ channels: Channel[] }>(`/api/v1/communities/${selectedCommunityId}/channels`),
      api<{ categories: Category[] }>(`/api/v1/communities/${selectedCommunityId}/categories`),
    ])
      .then(([channelResult, categoryResult]) => { if (alive) { setChannels(channelResult.channels); setCategories(categoryResult.categories) } })
      .catch((cause: unknown) => { if (alive) setError(cause instanceof Error ? cause.message : 'Could not load channels') })
    return () => { alive = false }
  }, [selectedCommunityId])

  useEffect(() => {
    activeIdRef.current = activeId
    lastSequenceRef.current = 0
    setReplyToMessage(null)
    if (!activeId) return
    let alive = true
    setOlderCursor(null)
    Promise.all([
      api<{ messages: Message[]; next_before: number | null }>(`/api/v1/conversations/${activeId}/messages?limit=50`),
      api<{ members: User[] }>(`/api/v1/conversations/${activeId}/members`),
      api<{ cursors: { user_id: string; sequence: number }[] }>(`/api/v1/conversations/${activeId}/read-cursors`),
      api<{ cursors: { user_id: string; sequence: number }[] }>(`/api/v1/conversations/${activeId}/delivery-cursors`),
    ])
      .then(async ([page, memberPage, cursorPage, deliveryPage]) => {
        if (!alive) return
        const chronological = [...page.messages].reverse()
        setMessages(chronological)
        setReactions({})
        setOlderCursor(page.next_before)
        setProfiles((current) => ({ ...current, ...Object.fromEntries(memberPage.members.map((member) => [member.id, member])) }))
        setReadCursors(Object.fromEntries(cursorPage.cursors.map((cursor) => [cursor.user_id, cursor.sequence])))
        setDeliveryCursors(Object.fromEntries(deliveryPage.cursors.map((cursor) => [cursor.user_id, cursor.sequence])))
        setLoadedConversationId(activeId)
        if (chronological.length) {
          const messageIds = chronological.map((message) => message.id)
          const reactionPage = await api<{ reactions: MessageReaction[] }>(`/api/v1/conversations/${activeId}/reactions?message_ids=${messageIds.join(',')}`)
          if (!alive) return
          replaceReactionsForMessages(messageIds, reactionPage.reactions)
        }
        lastSequenceRef.current = chronological.at(-1)?.sequence ?? 0
        if (lastSequenceRef.current > 0) recordDelivered(lastSequenceRef.current)
        if (lastSequenceRef.current > 0) void markRead(lastSequenceRef.current)
      })
      .catch((cause: unknown) => {
        if (!alive) return
        setLoadedConversationId(activeId)
        setMessages([])
        setError(cause instanceof Error ? cause.message : 'Could not load messages')
      })
    return () => { alive = false }
  }, [activeId, markRead, recordDelivered])

  useLayoutEffect(() => {
    if (!user || !activeId) { setDraft(''); return }
    setDraft(localStorage.getItem(`rustchat:draft:${user.id}:${activeId}`) ?? '')
  }, [activeId, user?.id])

  useEffect(() => {
    if (!user) return
    let stopped = false
    let retryTimer = 0
    let retryDelay = 800
    let socket: WebSocket
    const connect = () => {
      if (stopped) return
      const scheme = window.location.protocol === 'https:' ? 'wss:' : 'ws:'
      socket = new WebSocket(`${scheme}//${window.location.host}/api/v1/realtime`)
      socketRef.current = socket
      socket.onopen = async () => {
        retryDelay = 800
        setConnected(true)
        void loadCommunities().catch(() => {})
        const communityId = selectedCommunityIdRef.current
        if (communityId) {
          void Promise.all([
            api<{ channels: Channel[] }>(`/api/v1/communities/${communityId}/channels`),
            api<{ categories: Category[] }>(`/api/v1/communities/${communityId}/categories`),
          ]).then(([channelResult, categoryResult]) => {
            setChannels(channelResult.channels)
            setCategories(categoryResult.categories)
          }).catch(() => {})
          if (showCommunityManagerRef.current) {
            void api<{ members: User[] }>(`/api/v1/communities/${communityId}/members`).then(({ members }) => setCommunityMembers(members)).catch(() => {})
          }
        }
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
            api<{ cursors: { user_id: string; sequence: number }[] }>(`/api/v1/conversations/${conversationId}/read-cursors`),
            api<{ cursors: { user_id: string; sequence: number }[] }>(`/api/v1/conversations/${conversationId}/delivery-cursors`),
          ])
          if (activeIdRef.current === conversationId) {
            setReadCursors(Object.fromEntries(readPage.cursors.map((cursor) => [cursor.user_id, cursor.sequence])))
            setDeliveryCursors(Object.fromEntries(deliveryPage.cursors.map((cursor) => [cursor.user_id, cursor.sequence])))
          }
          const latestSequence = synced.messages.reduce((latest, message) => Math.max(latest, message.sequence), 0)
          if (latestSequence > 0) recordDelivered(latestSequence)
        } catch { /* A later reconnect can recover this page. */ }
        if (activeIdRef.current === conversationId && socket.readyState === WebSocket.OPEN) {
          socket.send(JSON.stringify({ type: 'subscribe', conversation_id: conversationId }))
        }
      }
      socket.onmessage = (event) => {
        try {
          const data = JSON.parse(event.data) as { type?: string; event_id?: string; conversation_id?: string; sequence?: number; occurred_at?: string; payload?: { message_id?: string; sender_id?: string; client_message_id?: string; reply_to_message_id?: string | null; mentioned_user_ids?: string[]; content?: string; edited_at?: string | null; deleted_at?: string | null; user_id?: string; sequence?: number; emoji?: string; online?: boolean | null; community_id?: string } }
          if ((data.type === 'members.updated' || data.type === 'channels.updated') && data.payload?.community_id) {
            const communityId = data.payload.community_id
            void loadCommunities().catch(() => {})
            if (data.type === 'channels.updated' && selectedCommunityIdRef.current === communityId) {
              void Promise.all([
                api<{ channels: Channel[] }>(`/api/v1/communities/${communityId}/channels`),
                api<{ categories: Category[] }>(`/api/v1/communities/${communityId}/categories`),
              ]).then(([channelResult, categoryResult]) => {
                setChannels(channelResult.channels)
                setCategories(categoryResult.categories)
              }).catch(() => {})
            }
            if (data.type === 'members.updated' && showCommunityManagerRef.current && selectedCommunityIdRef.current === communityId) {
              void api<{ members: User[] }>(`/api/v1/communities/${communityId}/members`).then(({ members }) => setCommunityMembers(members)).catch(() => {})
            }
            return
          }
          if (data.type === 'presence.updated' && data.payload?.user_id && data.payload.online !== undefined) {
            setPresence((current) => {
              if (data.payload!.online === null) { const next = { ...current }; delete next[data.payload!.user_id!]; return next }
              return { ...current, [data.payload!.user_id!]: data.payload!.online! }
            })
            return
          }
          if (data.type === 'delivery_ack' && data.sequence !== undefined && user?.id) {
            setDeliveryCursors((current) => ({ ...current, [user.id]: data.sequence! }))
            return
          }
          if (data.type === 'delivery_cursor.updated' && data.conversation_id === activeIdRef.current && data.payload?.user_id && typeof data.payload.sequence === 'number') {
            setDeliveryCursors((current) => ({ ...current, [data.payload!.user_id!]: data.payload!.sequence! }))
            return
          }
          if ((data.type === 'reaction.added' || data.type === 'reaction.removed') && data.event_id && data.conversation_id === activeIdRef.current && data.payload?.message_id && data.payload.emoji && data.payload.user_id) {
            if (seenReactionEventIdsRef.current.has(data.event_id)) return
            seenReactionEventIdsRef.current.add(data.event_id)
            if (seenReactionEventIdsRef.current.size > 1024) seenReactionEventIdsRef.current.delete(seenReactionEventIdsRef.current.values().next().value!)
            updateReactionCount(data.payload.message_id, data.payload.emoji, data.type === 'reaction.added')
            return
          }
          if (data.type === 'read_cursor.updated' && data.conversation_id === activeIdRef.current && data.payload?.user_id && typeof data.payload.sequence === 'number') {
            setReadCursors((current) => ({ ...current, [data.payload!.user_id!]: data.payload!.sequence! }))
            return
          }
          if ((data.type === 'typing.started' || data.type === 'typing.stopped') && data.conversation_id === activeIdRef.current && data.payload?.user_id) {
            if (typingExpiryRef.current !== null) window.clearTimeout(typingExpiryRef.current)
            setTypingUserId(data.type === 'typing.started' ? data.payload.user_id : null)
            if (data.type === 'typing.started') typingExpiryRef.current = window.setTimeout(() => setTypingUserId(null), 4500)
            return
          }
          if (!['message.created', 'message.updated', 'message.deleted'].includes(data.type ?? '') || !data.payload?.message_id || !data.payload.sender_id || !data.payload.client_message_id || data.payload.content === undefined || !data.conversation_id || data.conversation_id !== activeIdRef.current) return
          mergeMessages([{ id: data.payload.message_id, conversation_id: data.conversation_id, sender_id: data.payload.sender_id, sequence: data.sequence ?? 0, client_message_id: data.payload.client_message_id, reply_to_message_id: data.payload.reply_to_message_id, mentioned_user_ids: data.payload.mentioned_user_ids ?? [], content: data.payload.content, created_at: data.occurred_at ?? new Date().toISOString(), edited_at: data.payload.edited_at, deleted_at: data.payload.deleted_at }])
          if (data.sequence && data.sequence > 0) { recordDelivered(data.sequence); void markRead(data.sequence) }
        } catch { /* Ignore malformed frames. */ }
      }
      socket.onclose = () => {
        setConnected(false)
        setTypingUserId(null)
        if (!stopped) { retryTimer = window.setTimeout(connect, retryDelay); retryDelay = Math.min(retryDelay * 2, 15000) }
      }
      socket.onerror = () => socket.close()
    }
    connect()
    return () => { stopped = true; window.clearTimeout(retryTimer); if (typingExpiryRef.current !== null) window.clearTimeout(typingExpiryRef.current); socket?.close(); socketRef.current = null; setConnected(false) }
  }, [user, markRead, recordDelivered, updateReactionCount, loadCommunities])

  useEffect(() => {
    const socket = socketRef.current
    if (!socket || socket.readyState !== WebSocket.OPEN) return
    if (activeId) socket.send(JSON.stringify({ type: 'subscribe', conversation_id: activeId }))
  }, [activeId, connected])

  useLayoutEffect(() => {
    const container = messageScrollRef.current
    if (container && scrollHeightBeforePrependRef.current !== null) {
      container.scrollTop += container.scrollHeight - scrollHeightBeforePrependRef.current
      scrollHeightBeforePrependRef.current = null
    } else {
      messagesEndRef.current?.scrollIntoView?.({ behavior: 'smooth', block: 'end' })
    }
  }, [messages])

  function mergeMessages(incoming: Message[]) {
    if (!incoming.length) return
    setMessages((current) => {
      const byId = new Map(current.map((message) => [message.id, message]))
      for (const message of incoming) byId.set(message.id, message)
      const ordered = [...byId.values()].sort((a, b) => a.sequence - b.sequence)
      lastSequenceRef.current = Math.max(lastSequenceRef.current, ...incoming.map((message) => message.sequence))
      return ordered
    })
  }

  function replaceReactionsForMessages(messageIds: string[], incoming: MessageReaction[]) {
    setReactions((current) => {
      const next = { ...current }
      for (const id of messageIds) delete next[id]
      for (const reaction of incoming) next[reaction.message_id] = [...(next[reaction.message_id] ?? []), reaction]
      return next
    })
  }

  async function toggleReaction(message: Message, emoji: string) {
    const key = `${message.id}:${emoji}`
    if (pendingReactions[key] || !activeId) return
    const existing = reactions[message.id]?.find((reaction) => reaction.emoji === emoji)
    const method = existing?.reacted_by_me ? 'DELETE' : 'PUT'
    setPendingReactions((current) => ({ ...current, [key]: true }))
    try {
      const result = await api<{ reactions: MessageReaction[] }>(`/api/v1/conversations/${activeId}/messages/${message.id}/reactions`, { method, body: JSON.stringify({ emoji }) })
      replaceReactionsForMessages([message.id], result.reactions)
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : 'Could not update reaction')
    } finally {
      setPendingReactions((current) => { const next = { ...current }; delete next[key]; return next })
    }
  }

  async function submitAuth(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); setBusy(true); setAuthMessage(''); setError('')
    try {
      if (authMode === 'register') {
        await api('/api/v1/auth/register', { method: 'POST', body: JSON.stringify({ username, password, email: authEmail }) })
        setAuthMode('login'); setAuthMessage('Account created. Sign in with your new credentials.')
      } else {
        const result = await api<{ user: User }>('/api/v1/auth/login', { method: 'POST', body: JSON.stringify({ username, password }) })
        setUser(result.user); setPassword('')
      }
    } catch (cause) { setError(cause instanceof Error ? cause.message : 'Sign in failed') }
    finally { setBusy(false) }
  }

  async function submitPasswordRecovery(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); setBusy(true); setError(''); setAuthMessage('')
    try {
      if (recoveryMode === 'reset') {
        await api('/api/v1/auth/password-reset/complete', { method: 'POST', body: JSON.stringify({ token: recoveryToken, new_password: recoveryPassword }) })
        window.history.replaceState({}, '', '/')
        setRecoveryMode(null); setRecoveryPassword(''); setAuthMessage('Password updated. Sign in with your new password.')
      } else {
        await api('/api/v1/auth/password-reset/request', { method: 'POST', body: JSON.stringify({ email: authEmail }) })
        setAuthMessage('If an eligible account matches that address, a reset link will arrive by email.')
      }
    } catch (cause) { setError(cause instanceof Error ? cause.message : 'Could not complete password recovery') }
    finally { setBusy(false) }
  }

  async function signOut() {
    try { await api('/api/v1/auth/logout', { method: 'POST' }) } catch { /* Clear local state even if the session expired. */ }
    setUser(null); setProfile(null); setMessages([]); setError(''); setPresence({})
    setConversations([]); setActiveId(null); setLoadedConversationId(null)
    setCommunities([]); setSelectedCommunityId(null); setChannels([])
  }

  async function saveProfile(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!profile) return
    setProfileBusy(true)
    try {
      const result = await api<{ profile: UserProfile }>('/api/v1/auth/profile', {
        method: 'PATCH',
        body: JSON.stringify({ display_name: profile.display_name, bio: profile.bio }),
      })
      setProfile(result.profile)
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : 'Could not save profile')
    } finally {
      setProfileBusy(false)
    }
  }

  async function saveUsername() {
    if (!profile || !user) return
    setProfileBusy(true)
    setError('')
    try {
      const result = await api<{ user: User }>('/api/v1/auth/username', {
        method: 'PATCH',
        body: JSON.stringify({ username: profile.username }),
      })
      setUser(result.user)
      setProfile({ ...profile, username: result.user.username })
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : 'Could not update username')
    } finally {
      setProfileBusy(false)
    }
  }

  async function loadSessions() {
    try {
      const result = await api<{ sessions: Session[] }>('/api/v1/auth/sessions')
      setSessions(result.sessions)
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : 'Could not load active sessions')
    }
  }

  async function loadPrivacy() {
    try {
      const result = await api<{ privacy: ProfilePrivacy }>('/api/v1/auth/privacy')
      setPrivacy(result.privacy)
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : 'Could not load privacy settings')
    }
  }

  async function setInstanceUserSuspended(target: InstanceUser) {
    try {
      const { user: updated } = await api<{ user: InstanceUser }>(`/api/v1/admin/users/${target.id}/suspension`, {
        method: 'PUT',
        body: JSON.stringify({ suspended: !target.suspended_at }),
      })
      setInstanceUsers((current) => current?.map((item) => item.id === updated.id ? updated : item) ?? null)
      setAuthMessage(updated.suspended_at ? `${updated.username} suspended.` : `${updated.username} restored.`)
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : 'Could not update this account')
    }
  }

  async function savePrivacy(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!privacy) return
    setPrivacyBusy(true)
    try {
      const result = await api<{ privacy: ProfilePrivacy }>('/api/v1/auth/privacy', {
        method: 'PATCH', body: JSON.stringify(privacy),
      })
      setPrivacy(result.privacy)
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : 'Could not update privacy settings')
    } finally {
      setPrivacyBusy(false)
    }
  }

  async function changePassword(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    setPasswordBusy(true)
    setSecurityMessage('')
    try {
      await api('/api/v1/auth/password', {
        method: 'POST',
        body: JSON.stringify({ current_password: currentPassword, new_password: newPassword }),
      })
      setCurrentPassword('')
      setNewPassword('')
      setSecurityMessage('Password updated. Other sessions were signed out.')
      await loadSessions()
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : 'Could not change password')
    } finally {
      setPasswordBusy(false)
    }
  }



  async function manageAccount(action: 'deactivate' | 'delete') {
    if (!accountPassword || (action === 'delete' && deleteConfirmation !== 'DELETE')) return
    setAccountActionBusy(true)
    setError('')
    try {
      await api(`/api/v1/auth/${action}`, {
        method: 'POST',
        body: JSON.stringify({ current_password: accountPassword, ...(action === 'delete' ? { confirm: true } : {}) }),
      })
      setUser(null); setProfile(null); setMessages([]); setError(''); setPresence({})
      setConversations([]); setActiveId(null); setLoadedConversationId(null)
      setCommunities([]); setSelectedCommunityId(null); setChannels([])
      setAccountPassword(''); setDeleteConfirmation(''); setPassword('')
      setAuthMode('login')
      setAuthMessage(action === 'deactivate' ? 'Account deactivated. Sign in to reactivate it.' : 'Account deleted. Its messages remain in conversation history.')
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : `Could not ${action} account`)
    } finally {
      setAccountActionBusy(false)
    }
  }

  async function revokeSession(sessionId: string) {
    try {
      await api(`/api/v1/auth/sessions/${sessionId}`, { method: 'DELETE' })
      setSessions((current) => current.filter((session) => session.id !== sessionId))
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : 'Could not revoke session')
    }
  }

  async function createConversation(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); setCreating(true); setError('')
    const names = participants.split(/[\s,]+/).map((name) => name.trim()).filter(Boolean)
    try {
      const body = conversationKind === 'direct' ? { type: 'direct', username: names[0] } : { type: 'group', usernames: names }
      const result = await api<{ conversation: Conversation }>('/api/v1/conversations', { method: 'POST', body: JSON.stringify(body) })
      setConversations((current) => [result.conversation, ...current.filter((item) => item.id !== result.conversation.id)])
      setActiveId(result.conversation.id); setMobileList(false); setParticipants(''); setShowComposer(false)
    } catch (cause) { setError(cause instanceof Error ? cause.message : 'Could not create conversation') }
    finally { setCreating(false) }
  }

  async function previewUserProfile() {
    const username = participants.trim()
    if (!username || conversationKind !== 'direct') return
    setProfilePreviewBusy(true)
    setProfilePreviewError('')
    try {
      const result = await api<{ profile: UserProfile }>(`/api/v1/users/${encodeURIComponent(username)}/profile`)
      setProfilePreview(result.profile)
    } catch (cause) {
      setProfilePreview(null)
      setProfilePreviewError(cause instanceof Error ? cause.message : 'Profile is not available')
    } finally {
      setProfilePreviewBusy(false)
    }
  }

  function sendTyping(active: boolean) {
    const socket = socketRef.current
    if (activeId && socket?.readyState === WebSocket.OPEN) {
      socket.send(JSON.stringify({ type: 'typing', conversation_id: activeId, active }))
    }
  }

  async function sendMessage(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const content = draft.trim()
    if (!activeId || !content) return
    setDraft(''); setError('')
    try {
      const result = await api<{ message: Message }>(`/api/v1/conversations/${activeId}/messages`, { method: 'POST', body: JSON.stringify({ client_message_id: uuid7(), reply_to_message_id: replyToMessage?.id ?? null, content }) })
      if (user) localStorage.removeItem(`rustchat:draft:${user.id}:${activeId}`)
      setReplyToMessage(null)
      mergeMessages([result.message])
      void markRead(result.message.sequence)
    } catch (cause) {
      setDraft(content)
      if (user) localStorage.setItem(`rustchat:draft:${user.id}:${activeId}`, content)
      setError(cause instanceof Error ? cause.message : 'Message could not be sent')
    }
  }

  async function createCommunity(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    try {
      const result = await api<{ community: Community }>('/api/v1/communities', { method: 'POST', body: JSON.stringify({ name: communityName }) })
      setCommunities((current) => [...current, result.community])
      setSelectedCommunityId(result.community.id)
      setCommunityName(''); setShowCommunityComposer(false)
    } catch (cause) { setError(cause instanceof Error ? cause.message : 'Could not create community') }
  }

  async function joinCommunity(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    try {
      const result = await api<{ community: Community }>('/api/v1/communities/join', { method: 'POST', body: JSON.stringify({ invite_code: inviteCode.trim() }) })
      setCommunities((current) => [...current.filter((item) => item.id !== result.community.id), result.community])
      setSelectedCommunityId(result.community.id)
      setInviteCode(''); setShowJoinComposer(false)
    } catch (cause) { setError(cause instanceof Error ? cause.message : 'Could not join community') }
  }

  async function createCommunityInvite() {
    if (!selectedCommunityId) return
    try {
      const result = await api<{ invite_code: string }>(`/api/v1/communities/${selectedCommunityId}/invites`, { method: 'POST', body: JSON.stringify({ ttl_seconds: 604800, max_uses: 100 }) })
      setCreatedInviteCode(result.invite_code)
      setInviteCopied(false)
    } catch (cause) { setError(cause instanceof Error ? cause.message : 'Could not create invite') }
  }

  async function revokeCreatedInvite() {
    if (!selectedCommunityId || !createdInviteCode) return
    try {
      await api(`/api/v1/communities/${selectedCommunityId}/invites`, {
        method: 'DELETE',
        body: JSON.stringify({ invite_code: createdInviteCode }),
      })
      setCreatedInviteCode('')
      setInviteCopied(false)
    } catch (cause) { setError(cause instanceof Error ? cause.message : 'Could not revoke invite') }
  }

  async function revokeListedInvite(invite: CommunityInvite) {
    if (!selectedCommunityId || invite.revoked_at) return
    try {
      await api(`/api/v1/communities/${selectedCommunityId}/invites/${invite.id}`, { method: 'DELETE' })
      setCommunityInvites((current) => current.map((item) => item.id === invite.id ? { ...item, revoked_at: new Date().toISOString() } : item))
      if (createdInviteCode) setCreatedInviteCode('')
    } catch (cause) { setError(cause instanceof Error ? cause.message : 'Could not revoke invite') }
  }

  async function leaveCommunity() {
    if (!selectedCommunityId || !window.confirm('Leave this community?')) return
    try {
      await api(`/api/v1/communities/${selectedCommunityId}/members`, { method: 'DELETE' })
      const remaining = communities.filter((community) => community.id !== selectedCommunityId)
      setCommunities(remaining)
      setSelectedCommunityId(remaining[0]?.id ?? null)
      setChannels([])
      setActiveId(null)
    } catch (cause) { setError(cause instanceof Error ? cause.message : 'Could not leave community') }
  }

  async function createChannel(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!selectedCommunityId) return
    try {
      const result = await api<{ channel: Channel }>(`/api/v1/communities/${selectedCommunityId}/channels`, { method: 'POST', body: JSON.stringify({ name: channelName, category_id: channelCategoryId || null }) })
      setChannels((current) => [...current, result.channel].sort((a, b) => a.position - b.position))
      setActiveId(result.channel.conversation_id); setMobileList(false); setChannelName(''); setChannelCategoryId(''); setShowChannelComposer(false)
    } catch (cause) { setError(cause instanceof Error ? cause.message : 'Could not create channel') }
  }

  async function createCategory(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!selectedCommunityId) return
    try {
      const result = await api<{ category: Category }>(`/api/v1/communities/${selectedCommunityId}/categories`, { method: 'POST', body: JSON.stringify({ name: newCategoryName }) })
      setCategories((current) => [...current, result.category].sort((a, b) => a.position - b.position))
      setNewCategoryName('')
    } catch (cause) { setError(cause instanceof Error ? cause.message : 'Could not create category') }
  }

  async function renameCategory(category: Category) {
    const name = window.prompt('Category name', category.name)?.trim()
    if (!name || !selectedCommunityId) return
    try {
      const result = await api<{ category: Category }>(`/api/v1/communities/${selectedCommunityId}/categories/${category.id}`, { method: 'PATCH', body: JSON.stringify({ name, position: category.position }) })
      setCategories((current) => current.map((item) => item.id === category.id ? result.category : item))
    } catch (cause) { setError(cause instanceof Error ? cause.message : 'Could not rename category') }
  }

  async function deleteCategory(category: Category) {
    if (!selectedCommunityId || !window.confirm(`Delete category ${category.name}? Its channels will become uncategorized.`)) return
    try {
      await api(`/api/v1/communities/${selectedCommunityId}/categories/${category.id}`, { method: 'DELETE' })
      setCategories((current) => current.filter((item) => item.id !== category.id))
      setChannels((current) => current.map((channel) => channel.category_id === category.id ? { ...channel, category_id: null } : channel))
    } catch (cause) { setError(cause instanceof Error ? cause.message : 'Could not delete category') }
  }

  async function openCommunityManager() {
    if (!selectedCommunityId) return
    const community = communities.find((item) => item.id === selectedCommunityId)
    if (!community) return
    setCommunityEditName(community.name)
    setCommunityEditDescription(community.description)
    openModal(setShowCommunityManager)
    try {
      const [rolesResult, membersResult, invitesResult] = await Promise.all([
        api<{ roles: CommunityRole[] }>(`/api/v1/communities/${selectedCommunityId}/roles`),
        api<{ members: User[] }>(`/api/v1/communities/${selectedCommunityId}/members`),
        api<{ invites: CommunityInvite[] }>(`/api/v1/communities/${selectedCommunityId}/invites`),
      ])
      setCommunityRoles(rolesResult.roles)
      setCommunityMembers(membersResult.members)
      setCommunityInvites(invitesResult.invites)
      setSelectedRoleId(rolesResult.roles[0]?.id ?? '')
      setSelectedMemberId(membersResult.members.find((member) => member.id !== user?.id)?.id ?? '')
      setOverrideChannelId(channels[0]?.id ?? '')
    } catch (cause) { setError(cause instanceof Error ? cause.message : 'Could not load community management') }
  }

  async function saveCommunitySettings(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!selectedCommunityId) return
    try {
      const result = await api<{ community: Community }>(`/api/v1/communities/${selectedCommunityId}`, { method: 'PATCH', body: JSON.stringify({ name: communityEditName, description: communityEditDescription }) })
      setCommunities((current) => current.map((item) => item.id === result.community.id ? result.community : item))
    } catch (cause) { setError(cause instanceof Error ? cause.message : 'Could not update community') }
  }

  async function createRole(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!selectedCommunityId) return
    try {
      const result = await api<{ role: CommunityRole }>(`/api/v1/communities/${selectedCommunityId}/roles`, { method: 'POST', body: JSON.stringify({ name: newRoleName, permissions: newRolePermissions }) })
      setCommunityRoles((current) => [...current, result.role].sort((a, b) => a.position - b.position))
      setSelectedRoleId(result.role.id)
      setNewRoleName('')
    } catch (cause) { setError(cause instanceof Error ? cause.message : 'Could not create role') }
  }

  async function saveRole(role: CommunityRole) {
    if (!selectedCommunityId || role.is_default) return
    try {
      const result = await api<{ role: CommunityRole }>(`/api/v1/communities/${selectedCommunityId}/roles/${role.id}`, { method: 'PATCH', body: JSON.stringify({ name: role.name, permissions: role.permissions, position: role.position }) })
      setCommunityRoles((current) => current.map((item) => item.id === role.id ? result.role : item))
    } catch (cause) { setError(cause instanceof Error ? cause.message : 'Could not update role') }
  }

  async function deleteRole(role: CommunityRole) {
    if (!selectedCommunityId || role.is_default || !window.confirm(`Delete the ${role.name} role?`)) return
    try {
      await api(`/api/v1/communities/${selectedCommunityId}/roles/${role.id}`, { method: 'DELETE' })
      setCommunityRoles((current) => current.filter((item) => item.id !== role.id))
      if (selectedRoleId === role.id) setSelectedRoleId('')
    } catch (cause) { setError(cause instanceof Error ? cause.message : 'Could not delete role') }
  }

  async function assignSelectedRole(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!selectedCommunityId || !selectedRoleId || !selectedMemberId) return
    try {
      await api(`/api/v1/communities/${selectedCommunityId}/members/${selectedMemberId}/roles/${selectedRoleId}`, { method: 'POST' })
    } catch (cause) { setError(cause instanceof Error ? cause.message : 'Could not assign role') }
  }

  async function saveChannelOverride(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!selectedRoleId || !overrideChannelId) return
    try {
      await api(`/api/v1/channels/${overrideChannelId}/permissions/${selectedRoleId}`, { method: 'PUT', body: JSON.stringify({ allow: overrideAllow, deny: overrideDeny }) })
    } catch (cause) { setError(cause instanceof Error ? cause.message : 'Could not update channel permissions') }
  }

  async function editChannel(channel: Channel) {
    if (!selectedCommunityId) return
    const name = window.prompt('Channel name', channel.name)
    if (name === null) return
    try {
      const updated = await api<{ channel: Channel }>(`/api/v1/communities/${selectedCommunityId}/channels/${channel.id}`, { method: 'PATCH', body: JSON.stringify({ name, description: channel.description, read_only: channel.read_only, position: channel.position }) })
      setChannels((current) => current.map((item) => item.id === channel.id ? updated.channel : item))
    } catch (cause) { setError(cause instanceof Error ? cause.message : 'Could not rename channel') }
  }

  async function moveChannelCategory(channel: Channel, categoryId: string) {
    if (!selectedCommunityId) return
    try {
      const updated = await api<{ channel: Channel }>(`/api/v1/communities/${selectedCommunityId}/channels/${channel.id}`, {
        method: 'PATCH',
        body: JSON.stringify({ name: channel.name, description: channel.description, read_only: channel.read_only, position: channel.position, category_id: categoryId || null }),
      })
      setChannels((current) => current.map((item) => item.id === channel.id ? updated.channel : item))
    } catch (cause) { setError(cause instanceof Error ? cause.message : 'Could not move channel') }
  }

  async function deleteChannel(channel: Channel) {
    if (!selectedCommunityId || !window.confirm(`Delete #${channel.name} and its message history?`)) return
    try {
      await api(`/api/v1/communities/${selectedCommunityId}/channels/${channel.id}`, { method: 'DELETE' })
      setChannels((current) => current.filter((item) => item.id !== channel.id))
      if (activeId === channel.conversation_id) setActiveId(null)
    } catch (cause) { setError(cause instanceof Error ? cause.message : 'Could not delete channel') }
  }

  async function loadOlderMessages() {
    if (!activeId || olderCursor === null || loadingOlder) return
    setLoadingOlder(true)
    scrollHeightBeforePrependRef.current = messageScrollRef.current?.scrollHeight ?? null
    try {
      const page = await api<{ messages: Message[]; next_before: number | null }>(`/api/v1/conversations/${activeId}/messages?before_sequence=${olderCursor}&limit=50`)
      setMessages((current) => {
        const byId = new Map(current.map((message) => [message.id, message]))
        for (const message of page.messages) byId.set(message.id, message)
        return [...byId.values()].sort((a, b) => a.sequence - b.sequence)
      })
      setOlderCursor(page.next_before)
    } catch (cause) {
      scrollHeightBeforePrependRef.current = null
      setError(cause instanceof Error ? cause.message : 'Could not load older messages')
    } finally { setLoadingOlder(false) }
  }

  const activeConversation = conversations.find((conversation) => conversation.id === activeId)
  const activeChannel = channels.find((channel) => channel.conversation_id === activeId)
  const activeMessages = loadedConversationId === activeId ? messages : []
  const loadingMessages = activeId !== null && loadedConversationId !== activeId
  const selectedChannels = channels.filter((channel) => channel.community_id === selectedCommunityId)
  const channelGroups = [
    ...categories.map((category) => ({ id: category.id, label: category.name, channels: selectedChannels.filter((channel) => channel.category_id === category.id) })),
    { id: 'uncategorized', label: 'UNCATEGORIZED', channels: selectedChannels.filter((channel) => !channel.category_id) },
  ]
  const viewerId = user?.id ?? ''
  const activeTitle = activeChannel?.name ?? (activeConversation
    ? activeConversation.kind === 'direct'
      ? activeConversation.members.filter((memberId) => memberId !== viewerId).map((memberId) => profiles[memberId]?.username).find(Boolean) ?? displayName(activeConversation, viewerId)
      : displayName(activeConversation, viewerId)
    : 'Your messages')

  if (checkingSession) return <main className="loading-screen"><span className="brand-mark">R</span><p>Opening RustChat…</p></main>
  if (!user) return (
    <main className="auth-screen">
      <section className="auth-intro"><div className="brand-lockup"><span className="brand-mark">R</span><span>RustChat</span></div><p className="eyebrow">A quieter place to talk</p><h1>Good conversations<br />start here.</h1><p className="intro-copy">A self-hosted space for the people and communities you care about.</p><div className="intro-rule"><span />PRIVATE BY DESIGN</div></section>
      <section className="auth-panel"><div className="auth-form-wrap"><p className="eyebrow">YOUR SPACE</p><h2>{recoveryMode === 'reset' ? 'Choose a new password' : recoveryMode === 'request' ? 'Reset your password' : authMode === 'login' ? 'Welcome back' : 'Create your account'}</h2><p className="muted">{recoveryMode ? 'Use the secure link sent to your email address.' : authMode === 'login' ? 'Sign in to pick up where you left off.' : 'Choose a username, email address, and a strong password.'}</p>
        {error && <div className="notice error" role="alert">{error}</div>}{authMessage && <div className="notice success" role="status">{authMessage}</div>}
        {recoveryMode ? <form className="stack-form" onSubmit={submitPasswordRecovery}>{recoveryMode === 'request' ? <label>Email<input autoComplete="email" type="email" required value={authEmail} onChange={(event) => setAuthEmail(event.target.value)} /></label> : <label>New password<input type="password" autoComplete="new-password" minLength={12} maxLength={128} required value={recoveryPassword} onChange={(event) => setRecoveryPassword(event.target.value)} /></label>}<button className="primary-button" disabled={busy}>{busy ? 'Please wait…' : recoveryMode === 'request' ? 'Send reset link' : 'Update password'}</button><button type="button" className="text-button" onClick={() => { setRecoveryMode(null); setError('') }}>Back to sign in</button></form> : <><form className="stack-form" onSubmit={submitAuth}><label>Username<input autoComplete="username" minLength={3} maxLength={32} required value={username} onChange={(event) => setUsername(event.target.value)} /></label>{authMode === 'register' && <label>Email<input autoComplete="email" type="email" required value={authEmail} onChange={(event) => setAuthEmail(event.target.value)} /></label>}<label>Password<input type="password" autoComplete={authMode === 'login' ? 'current-password' : 'new-password'} minLength={12} maxLength={128} required value={password} onChange={(event) => setPassword(event.target.value)} /></label><button className="primary-button" disabled={busy}>{busy ? 'Please wait…' : authMode === 'login' ? 'Sign in' : 'Create account'}</button></form>{authMode === 'login' && <button className="text-button" onClick={() => { setRecoveryMode('request'); setAuthMessage(''); setError('') }}>Forgot password?</button>}<p className="switch-auth">{authMode === 'login' ? 'New to this space?' : 'Already have an account?'} <button className="text-button" onClick={() => { setAuthMode(authMode === 'login' ? 'register' : 'login'); setError(''); setAuthMessage('') }}>{authMode === 'login' ? 'Create an account' : 'Sign in'}</button></p></>}
      </div><div className="auth-foot"><span>RUSTCHAT · SELF-HOSTED MESSAGING</span><span>01 / 01</span></div></section>
    </main>
  )

  return <main className="workspace">
    <aside className="workspace-rail"><div className="rail-brand" aria-label="RustChat">R</div><div className="rail-divider" /><div className="rail-avatar" title={user.username}>{user.username.slice(0, 1).toUpperCase()}</div><div className="rail-spacer" /><button className="rail-account" onClick={signOut} aria-label={`Sign out ${user.username}`} title="Sign out">↗</button></aside>
    <aside className={`conversation-sidebar ${mobileList ? 'mobile-visible' : ''}`}><header className="sidebar-header"><div><p className="eyebrow">YOUR WORKSPACE</p><h1>Messages</h1></div><button className="new-button" onClick={() => openModal(setShowComposer)} aria-label="New conversation" title="New conversation">+</button></header><div className="sidebar-search"><span aria-hidden="true">⌕</span><span>Conversations</span><kbd>{conversationShortcut}</kbd></div><div className="community-area"><div className="section-heading"><span>COMMUNITIES</span><button onClick={() => openModal(setShowCommunityComposer)} aria-label="Create community" title="Create community">+</button><button className="join-community-button" onClick={() => openModal(setShowJoinComposer)} aria-label="Join community" title="Join community">Join</button></div>{communities.map((community) => <button key={community.id} className={"community-item " + (community.id === selectedCommunityId ? "selected" : "")} onClick={() => { setSelectedCommunityId(community.id); setActiveId(null) }}><span className="community-avatar">{community.name.slice(0, 1).toUpperCase()}</span><span>{community.name}</span><small>{community.member_count}</small></button>)}{selectedCommunityId && <div className="community-actions"><button onClick={createCommunityInvite}>Create invite</button><button onClick={openCommunityManager}>Manage</button><button onClick={leaveCommunity}>Leave</button></div>}{createdInviteCode && <div className="invite-code" role="status"><span>Invite code: {createdInviteCode}</span><button onClick={() => navigator.clipboard.writeText(createdInviteCode).then(() => setInviteCopied(true))}>Copy</button>{inviteCopied && <small role="status">Copied</small>}<button onClick={() => void revokeCreatedInvite()}>Revoke</button></div>}{selectedCommunityId && <><div className="section-heading channels-heading"><span>CHANNELS</span><button onClick={() => openModal(setShowChannelComposer)} aria-label="Create channel" title="Create channel">+</button></div>{channelGroups.filter((group) => group.channels.length > 0).map((group) => <div className="channel-group" key={group.id}><span className="channel-group-label">{group.label}</span>{group.channels.map((channel) => <button key={channel.id} className={"channel-item " + (activeId === channel.conversation_id ? "selected" : "")} onClick={() => { setActiveId(channel.conversation_id); setMobileList(false) }}><span>#</span>{channel.name}</button>)}</div>)}</>}</div><div className="section-heading dm-heading"><span>DIRECT MESSAGES</span><button onClick={() => openModal(setShowComposer)} aria-label="New direct message">+</button></div><div className="conversation-list" aria-label="Conversations">{conversations.length === 0 ? <div className="sidebar-empty"><span className="empty-glyph">◌</span><strong>No conversations yet</strong><span>Start a direct message or bring a group together.</span><button className="outline-button" onClick={() => openModal(setShowComposer)}>Start a conversation</button></div> : conversations.map((conversation, index) => <button key={conversation.id} className={`conversation-item ${activeId === conversation.id ? 'selected' : ''}`} onClick={() => { setActiveId(conversation.id); setMobileList(false); setError('') }}><span className={`conversation-avatar ${conversation.kind}`}>{conversation.kind === 'group' ? 'G' : shortId(conversation.members.find((member) => member !== user.id) ?? conversation.id).slice(0, 1).toUpperCase()}</span><span className="conversation-summary"><strong>{displayName(conversation, user.id, profiles)}</strong>{conversation.kind === 'direct' && conversation.members.some((member) => member !== user.id && presence[member]) && <i className="presence-dot" role="img" aria-label="Online" title="Online" />}<span>{conversation.kind === 'group' ? 'Group' : 'Direct message'} <span className="dot-separator">·</span> {new Date(conversation.created_at).toLocaleDateString()}</span></span>{index === 0 && <i className="unread-dot" />}</button>)}</div><footer className="sidebar-profile"><div className="profile-avatar">{user.username.slice(0, 1).toUpperCase()}</div><div className="profile-copy"><strong>{profile?.display_name || user.username}</strong><span>@{user.username}</span></div><details className="profile-settings"><summary aria-label="Edit profile" title="Edit profile">Edit</summary>{profile && <form className="profile-form" onSubmit={saveProfile}><label>Username<input minLength={3} maxLength={32} pattern="[A-Za-z0-9_]+" required value={profile.username} onChange={(event) => setProfile({ ...profile, username: event.target.value })} /></label><button type="button" className="outline-button" disabled={profileBusy || profile.username === user.username} onClick={() => void saveUsername()}>Update username</button><label>Display name<input maxLength={64} value={profile.display_name} onChange={(event) => setProfile({ ...profile, display_name: event.target.value })} /></label><label>About you<textarea maxLength={280} rows={3} value={profile.bio} onChange={(event) => setProfile({ ...profile, bio: event.target.value })} /></label><button className="primary-button" disabled={profileBusy}>{profileBusy ? "Saving?" : "Save profile"}</button></form>}</details><details className="security-settings"><summary>Appearance</summary><section><label>Theme<select aria-label="Theme" value={theme} onChange={(event) => setTheme(event.target.value as "light" | "dark")}><option value="light">Light</option><option value="dark">Dark</option></select></label></section></details><details className="security-settings" onToggle={(event) => { if (event.currentTarget.open) void loadSessions() }}><summary>Security</summary><section><form className="security-form" onSubmit={changePassword}><label>Current password<input type="password" autoComplete="current-password" maxLength={128} required value={currentPassword} onChange={(event) => setCurrentPassword(event.target.value)} /></label><label>New password<input type="password" autoComplete="new-password" minLength={12} maxLength={128} required value={newPassword} onChange={(event) => setNewPassword(event.target.value)} /></label><button className="primary-button" disabled={passwordBusy}>{passwordBusy ? "Updating..." : "Change password"}</button></form>{securityMessage && <p className="notice success" role="status">{securityMessage}</p>}<h4>Active sessions</h4>{sessions.length === 0 ? <p className="muted">No active sessions found.</p> : sessions.map((session) => <div className="session-row" key={session.id}><span><strong>{session.current ? "This session" : "Signed in"}</strong><small>Started {new Date(session.created_at).toLocaleString()}</small></span>{session.current ? <span className="current-session">Current</span> : <button type="button" onClick={() => void revokeSession(session.id)}>Revoke</button>}</div>)}</section></details><details className="security-settings"><summary>Account actions</summary><section><p className="muted">Deactivation signs out all sessions. You can reactivate by signing in again. Transfer community ownership before either action.</p><label>Current password<input type="password" autoComplete="current-password" maxLength={128} value={accountPassword} onChange={(event) => setAccountPassword(event.target.value)} /></label><button type="button" className="outline-button" disabled={!accountPassword || accountActionBusy} onClick={() => void manageAccount('deactivate')}>{accountActionBusy ? 'Working…' : 'Deactivate account'}</button><label>Type DELETE to permanently delete your account<input autoComplete="off" value={deleteConfirmation} onChange={(event) => setDeleteConfirmation(event.target.value)} /></label><button type="button" className="account-delete-button" disabled={!accountPassword || deleteConfirmation !== 'DELETE' || accountActionBusy} onClick={() => void manageAccount('delete')}>Delete account and scrub profile</button></section></details><details className="security-settings" onToggle={(event) => { if (event.currentTarget.open) void loadPrivacy() }}><summary>Privacy</summary><section>{privacy ? <form className="security-form" onSubmit={savePrivacy}><label className="privacy-toggle"><input type="checkbox" checked={privacy.discoverable} onChange={(event) => setPrivacy({ ...privacy, discoverable: event.target.checked })} />Allow people to find me by username</label><label className="privacy-toggle"><input type="checkbox" checked={privacy.presence_visible} onChange={(event) => setPrivacy({ ...privacy, presence_visible: event.target.checked })} />Show my online status to conversation members</label><label>Who can view my profile<select value={privacy.profile_visibility} onChange={(event) => setPrivacy({ ...privacy, profile_visibility: event.target.value as ProfilePrivacy["profile_visibility"] })}><option value="everyone">Everyone who can find me</option><option value="conversation_members">People in my conversations</option></select></label><button className="primary-button" disabled={privacyBusy}>{privacyBusy ? "Saving..." : "Save privacy"}</button></form> : <p className="muted">Loading privacy settings...</p>}</section></details>{instanceUsers && <details className="security-settings"><summary>Instance administration</summary><section><p className="muted">The newest 100 accounts are shown. Suspension revokes sessions and prevents sign-in.</p><div className="instance-user-list">{instanceUsers.map((item) => <div className="manager-row" key={item.id}><span>{item.username} · {item.deleted_at ? "Deleted" : item.suspended_at ? "Suspended" : item.deactivated_at ? "Deactivated" : "Active"}</span>{item.is_instance_admin ? <small>Configured administrator</small> : !item.deleted_at && <button type="button" aria-label={`${item.suspended_at ? "Restore" : "Suspend"} ${item.username}`} onClick={() => void setInstanceUserSuspended(item)}>{item.suspended_at ? "Restore" : "Suspend"}</button>}</div>)}</div></section></details>}<button className="signout-button" onClick={signOut}>Sign out</button></footer></aside>
    <section className={`chat-panel ${mobileList ? 'mobile-hidden' : ''}`}><header className="chat-header">{activeConversation || activeChannel ? <><button className="mobile-back" onClick={() => setMobileList(true)} aria-label="Back to conversations">←</button><div className="chat-title-avatar">{activeChannel ? '#' : activeConversation?.kind === 'group' ? 'G' : 'D'}</div><div className="chat-title"><h2>{activeTitle}</h2><p>{activeConversation?.kind === 'group' ? activeConversation.members.length + ' members' : activeChannel ? selectedChannels.length + ' channels Â· ' + (communities.find((item) => item.id === selectedCommunityId)?.name ?? 'Community') : 'Private conversation'}</p></div><div className="connection-status" role="status" aria-live="polite"><span className={connected ? 'status-dot online' : 'status-dot'} />{connected ? 'Live' : 'Reconnecting'}</div><button className="more-button" aria-label="Conversation details">···</button></> : <div className="chat-title"><h2>Your messages</h2><p>A space for thoughtful conversations</p></div>}</header>
      {error && <div className="inline-error" role="alert">{error}<button onClick={() => setError('')} aria-label="Dismiss">×</button></div>}
      {activeConversation || activeChannel ? <><div className="message-scroll" ref={messageScrollRef} role="log" aria-label={`Messages in ${activeTitle}`} aria-live="polite" aria-relevant="additions" aria-busy={loadingMessages}>{olderCursor !== null && <button className="load-older-button" onClick={loadOlderMessages} disabled={loadingOlder}>{loadingOlder ? 'Loading older messages…' : 'Load older messages'}</button>}{olderCursor === null && <div className="history-start"><span className="history-line" /><span>THIS IS THE BEGINNING</span><span className="history-line" /></div>}{loadingMessages ? <div className="message-placeholder">Loading messages…</div> : activeMessages.length === 0 ? <div className="message-placeholder"><span className="empty-glyph">◌</span><strong>A fresh conversation</strong><span>Send a message to get things started.</span></div> : activeMessages.map((message, index) => { const own = message.sender_id === user.id; const prior = activeMessages[index - 1]; const grouped = prior?.sender_id === message.sender_id && new Date(message.created_at).getTime() - new Date(prior.created_at).getTime() < 300000; const deliveredTo = Object.entries(deliveryCursors).filter(([reader, sequence]) => reader !== user.id && sequence >= message.sequence).length; const readBy = Object.entries(readCursors).filter(([reader, sequence]) => reader !== user.id && sequence >= message.sequence).length; return <article className={`message-row ${own ? 'own' : ''} ${grouped ? 'grouped' : ''}`} key={message.id}><div className="message-avatar">{own ? user.username.slice(0, 1).toUpperCase() : shortId(message.sender_id).slice(0, 1).toUpperCase()}</div><div className="message-content">{!grouped && <div className="message-meta"><strong>{own ? user.username : `${profiles[message.sender_id]?.username ?? `Member ${shortId(message.sender_id)}`}`}</strong><time>{timeLabel(message.created_at)}</time></div>}<>{message.reply_to_message_id && <blockquote className="reply-quote">{activeMessages.find((item) => item.id === message.reply_to_message_id)?.content ?? "Original message"}</blockquote>}<div className="message-body">{renderMessageContent(message, profiles)}</div></>{!message.deleted_at && <div className="message-reactions" aria-label="Reactions">{['👍', '❤️', '😂', '🎉', '😮', '😢'].map((emoji) => { const reaction = reactions[message.id]?.find((item) => item.emoji === emoji); const pendingKey = `${message.id}:${emoji}`; return <button key={emoji} type="button" className={reaction?.reacted_by_me ? 'selected' : ''} aria-pressed={reaction?.reacted_by_me ?? false} aria-label={`${reaction?.reacted_by_me ? 'Remove' : 'Add'} ${emoji} reaction${reaction ? `, ${reaction.count} total` : ''}`} disabled={pendingReactions[pendingKey]} onClick={() => void toggleReaction(message, emoji)}>{emoji}{reaction && <span>{reaction.count}</span>}</button> })}</div>}<button className="reply-button" type="button" onClick={() => setReplyToMessage(message)}>Reply</button>{own && deliveredTo > 0 && <small>Delivered to {deliveredTo}</small>}{own && deliveredTo === 0 && <small>Sent</small>}{own && readBy > 0 && <small>Seen by {readBy}</small>}{!message.deleted_at && own && <div className="message-actions"><button onClick={async () => { const updated = window.prompt("Edit message", message.content); if (updated !== null && updated.trim()) { try { const result = await api<{ message: Message }>(`/api/v1/conversations/${activeId}/messages/${message.id}`, { method: "PATCH", body: JSON.stringify({ content: updated.trim() }) }); mergeMessages([result.message]) } catch (cause) { setError(cause instanceof Error ? cause.message : "Could not edit message") } } }}>Edit</button><button onClick={async () => { if (window.confirm("Delete this message?")) { try { const result = await api<{ message: Message }>(`/api/v1/conversations/${activeId}/messages/${message.id}`, { method: "DELETE" }); mergeMessages([result.message]) } catch (cause) { setError(cause instanceof Error ? cause.message : "Could not delete message") } } }}>Delete</button></div>}{message.edited_at && <small>edited</small>}</div></article> })}<div ref={messagesEndRef} />{typingUserId && <div className="typing-indicator" role="status">{profiles[typingUserId]?.username ?? 'A member'} is typing?</div>}</div><form className="message-composer" onSubmit={sendMessage}>{replyToMessage && <div className="replying-to"><span>Replying to {profiles[replyToMessage.sender_id]?.username ?? "member"}: {replyToMessage.content}</span><button type="button" onClick={() => setReplyToMessage(null)} aria-label="Cancel reply">?</button></div>}<textarea aria-label="Write a message" placeholder={activeChannel ? 'Message #' + activeChannel.name : 'Message ' + activeTitle} disabled={activeChannel?.read_only} rows={1} maxLength={4000} value={draft} onChange={(event) => { setDraft(event.target.value); if (user && activeId) { if (event.target.value) localStorage.setItem(`rustchat:draft:${user.id}:${activeId}`, event.target.value); else localStorage.removeItem(`rustchat:draft:${user.id}:${activeId}`) } if (event.target.value.trim()) sendTyping(true) }} onBlur={() => sendTyping(false)} onKeyDown={(event) => { if (event.key === 'Enter' && !event.shiftKey) { event.preventDefault(); event.currentTarget.form?.requestSubmit() } }} /><div className="composer-footer"><span><kbd>Enter</kbd> to send <span className="dot-separator">·</span> <kbd>Shift</kbd> + <kbd>Enter</kbd> for a new line</span><button className="send-button" disabled={!draft.trim() || activeChannel?.read_only} aria-label="Send message">↑</button></div></form></> : <div className="welcome-panel"><span className="welcome-mark">R</span><p className="eyebrow">A GOOD PLACE TO BEGIN</p><h2>Your conversations,<br />all in one place.</h2><p>Choose a conversation from the left, or start a new one to say hello.</p><button className="primary-button compact" onClick={() => openModal(setShowComposer)}>Start a conversation <span>↗</span></button></div>}
      <footer className="chat-footer"><span>RUSTCHAT</span><span>Messages are private to their members</span></footer>
    </section>
    {showComposer && <div className="modal-backdrop" onMouseDown={(event) => { if (event.target === event.currentTarget) setShowComposer(false) }}><section className="dialog" role="dialog" aria-modal="true" aria-labelledby="dialog-title"><button className="dialog-close" onClick={() => setShowComposer(false)} aria-label="Close">×</button><p className="eyebrow">NEW MESSAGE</p><h2 id="dialog-title">Start a conversation</h2><p className="muted">Find people by their exact username.</p><div className="kind-switch" role="group" aria-label="Conversation type"><button className={conversationKind === 'direct' ? 'active' : ''} onClick={() => setConversationKind('direct')}>Direct</button><button className={conversationKind === 'group' ? 'active' : ''} onClick={() => setConversationKind('group')}>Group</button></div><form className="stack-form" onSubmit={createConversation}><label>{conversationKind === 'direct' ? 'Username' : 'Usernames'}<input autoFocus required value={participants} placeholder={conversationKind === 'direct' ? 'e.g. riverstone' : 'Separate usernames with commas'} onChange={(event) => { setParticipants(event.target.value); setProfilePreview(null); setProfilePreviewError('') }} /></label>{conversationKind === 'direct' && <><button type="button" className="outline-button" disabled={!participants.trim() || profilePreviewBusy} onClick={() => void previewUserProfile()}>{profilePreviewBusy ? 'Loading profile...' : 'View profile'}</button>{profilePreview && <div className="profile-preview"><strong>{profilePreview.display_name || profilePreview.username}</strong><span>@{profilePreview.username}</span>{profilePreview.bio && <p>{profilePreview.bio}</p>}</div>}{profilePreviewError && <p className="inline-error" role="status">{profilePreviewError}</p>}</>}{conversationKind === 'group' && <span className="field-hint">You’ll be included automatically. Up to 100 people.</span>}<button className="primary-button" disabled={creating}>{creating ? 'Creating…' : 'Continue'}</button></form></section></div>}
    {showCommunityComposer && <div className="modal-backdrop"><section className="dialog" role="dialog" aria-modal="true" aria-labelledby="community-title"><button className="dialog-close" onClick={() => setShowCommunityComposer(false)} aria-label="Close">×</button><p className="eyebrow">NEW COMMUNITY</p><h2 id="community-title">Create a community</h2><p className="muted">Give your group a name. You can invite people next.</p><form className="stack-form" onSubmit={createCommunity}><label>Community name<input autoFocus required minLength={2} maxLength={80} value={communityName} onChange={(event) => setCommunityName(event.target.value)} /></label><button className="primary-button">Create community</button></form></section></div>}
    {showJoinComposer && <div className="modal-backdrop"><section className="dialog" role="dialog" aria-modal="true" aria-labelledby="join-title"><button className="dialog-close" onClick={() => setShowJoinComposer(false)} aria-label="Close">×</button><p className="eyebrow">JOIN A COMMUNITY</p><h2 id="join-title">Enter an invite code</h2><form className="stack-form" onSubmit={joinCommunity}><label>Invite code<input autoFocus required value={inviteCode} onChange={(event) => setInviteCode(event.target.value)} /></label><button className="primary-button">Join community</button></form></section></div>}
    {showCommunityManager && selectedCommunityId && <div className="modal-backdrop" onMouseDown={(event) => { if (event.target === event.currentTarget) setShowCommunityManager(false) }}><section className="dialog community-manager" role="dialog" aria-modal="true" aria-labelledby="manage-community-title"><button className="dialog-close" onClick={() => setShowCommunityManager(false)} aria-label="Close community management">?</button><p className="eyebrow">COMMUNITY SETTINGS</p><h2 id="manage-community-title">Manage {communities.find((item) => item.id === selectedCommunityId)?.name}</h2>
      <section className="manager-section"><h3>Invitations</h3>{communityInvites.length === 0 ? <p className="muted">No invitations have been created.</p> : communityInvites.map((invite) => <div className="manager-row" key={invite.id}><span>{invite.revoked_at ? 'Revoked' : invite.expires_at && Date.parse(invite.expires_at) <= Date.now() ? 'Expired' : invite.max_uses !== null && invite.use_count >= invite.max_uses ? 'Used up' : 'Active'} · {invite.use_count}{invite.max_uses === null ? '' : `/${invite.max_uses}`} uses{invite.expires_at ? ` · expires ${new Date(invite.expires_at).toLocaleDateString()}` : ''}</span>{!invite.revoked_at && <button type="button" onClick={() => void revokeListedInvite(invite)}>Revoke</button>}</div>)}</section>
      <form className="stack-form" onSubmit={saveCommunitySettings}><label>Name<input required maxLength={80} value={communityEditName} onChange={(event) => setCommunityEditName(event.target.value)} /></label><label>Description<textarea maxLength={280} rows={2} value={communityEditDescription} onChange={(event) => setCommunityEditDescription(event.target.value)} /></label><button className="primary-button">Save community</button></form>
      <section className="manager-section"><h3>Categories</h3><form className="role-create-form" onSubmit={createCategory}><label>New category<input required maxLength={64} value={newCategoryName} onChange={(event) => setNewCategoryName(event.target.value)} placeholder="Projects" /></label><button className="outline-button">Create category</button></form>{categories.map((category) => <div className="manager-row" key={category.id}><span>{category.name}</span><button type="button" onClick={() => void renameCategory(category)}>Rename</button><button type="button" onClick={() => void deleteCategory(category)}>Delete</button></div>)}</section><section className="manager-section"><h3>Channels</h3>{selectedChannels.length === 0 ? <p className="muted">No channels yet.</p> : selectedChannels.map((channel) => <div className="manager-row" key={channel.id}><span>#{channel.name}</span><select aria-label={`Category for ${channel.name}`} value={channel.category_id ?? ""} onChange={(event) => void moveChannelCategory(channel, event.target.value)}><option value="">Uncategorized</option>{categories.map((category) => <option key={category.id} value={category.id}>{category.name}</option>)}</select><button type="button" onClick={() => editChannel(channel)}>Rename</button><button type="button" onClick={() => deleteChannel(channel)}>Delete</button></div>)}</section>
      <section className="manager-section"><h3>Members</h3>{communityMembers.map((member) => <div className="manager-row" key={member.id}><span>{member.username}{member.id === communities.find((item) => item.id === selectedCommunityId)?.owner_id ? ' ? owner' : ''}</span>{member.id !== user.id && member.id !== communities.find((item) => item.id === selectedCommunityId)?.owner_id && <button type="button" onClick={async () => { if (!window.confirm(`Remove ${member.username} from this community?`)) return; try { await api(`/api/v1/communities/${selectedCommunityId}/members/${member.id}`, { method: 'DELETE' }); setCommunityMembers((current) => current.filter((item) => item.id !== member.id)); if (selectedMemberId === member.id) setSelectedMemberId('') } catch (cause) { setError(cause instanceof Error ? cause.message : 'Could not remove member') } }}>Remove</button>}</div>)}</section>
      <section className="manager-section"><h3>Roles and permissions</h3><form className="role-create-form" onSubmit={createRole}><label>New role<input required minLength={2} maxLength={32} value={newRoleName} onChange={(event) => setNewRoleName(event.target.value)} /></label><div className="permission-grid">{ROLE_PERMISSIONS.map(([label, bit]) => <label key={bit}><input type="checkbox" checked={(newRolePermissions & bit) !== 0} onChange={(event) => setNewRolePermissions((current) => event.target.checked ? current | bit : current & ~bit)} />{label}</label>)}</div><button className="outline-button">Create role</button></form>
        {communityRoles.map((role) => <article className="role-editor" key={role.id}><div className="role-editor-heading"><strong>{role.name}{role.is_default ? ' ? default' : ''}</strong>{!role.is_default && <><button type="button" onClick={() => void saveRole(role)}>Save role</button><button type="button" onClick={() => void deleteRole(role)}>Delete role</button></>}</div>{!role.is_default && <><label>Role name<input value={role.name} maxLength={32} onChange={(event) => setCommunityRoles((current) => current.map((item) => item.id === role.id ? { ...item, name: event.target.value } : item))} /></label><label>Position<input type="number" min={1} value={role.position} onChange={(event) => setCommunityRoles((current) => current.map((item) => item.id === role.id ? { ...item, position: Number(event.target.value) } : item))} /></label><div className="permission-grid">{ROLE_PERMISSIONS.map(([label, bit]) => <label key={bit}><input type="checkbox" checked={(role.permissions & bit) !== 0} onChange={(event) => setCommunityRoles((current) => current.map((item) => item.id === role.id ? { ...item, permissions: event.target.checked ? item.permissions | bit : item.permissions & ~bit } : item))} />{label}</label>)}</div></>}</article>)}
        <form className="role-assign-form" onSubmit={assignSelectedRole}><h4>Assign a role</h4><label>Member<select required value={selectedMemberId} onChange={(event) => setSelectedMemberId(event.target.value)}><option value="">Choose member</option>{communityMembers.filter((member) => member.id !== user.id).map((member) => <option key={member.id} value={member.id}>{member.username}</option>)}</select></label><label>Role<select required value={selectedRoleId} onChange={(event) => setSelectedRoleId(event.target.value)}>{communityRoles.map((role) => <option key={role.id} value={role.id}>{role.name}</option>)}</select></label><button className="outline-button">Assign role</button><button type="button" className="outline-button" onClick={async () => { if (!selectedMemberId || !selectedRoleId) return; try { await api(`/api/v1/communities/${selectedCommunityId}/members/${selectedMemberId}/roles/${selectedRoleId}`, { method: 'DELETE' }) } catch (cause) { setError(cause instanceof Error ? cause.message : 'Could not remove role') } }}>Remove role</button></form>
        <form className="role-assign-form" onSubmit={saveChannelOverride}><h4>Channel permission override</h4><label>Channel<select required value={overrideChannelId} onChange={(event) => setOverrideChannelId(event.target.value)}>{selectedChannels.map((channel) => <option key={channel.id} value={channel.id}>{channel.name}</option>)}</select></label><label>Role<select required value={selectedRoleId} onChange={(event) => setSelectedRoleId(event.target.value)}>{communityRoles.map((role) => <option key={role.id} value={role.id}>{role.name}</option>)}</select></label><label>Allow bits<input type="number" min={0} max={511} value={overrideAllow} onChange={(event) => setOverrideAllow(Number(event.target.value))} /></label><label>Deny bits<input type="number" min={0} max={511} value={overrideDeny} onChange={(event) => setOverrideDeny(Number(event.target.value))} /></label><button className="outline-button">Save override</button></form>
      </section>
      {communities.find((item) => item.id === selectedCommunityId)?.owner_id === user.id && <section className="manager-section"><h3>Ownership</h3><label>Transfer to<select value={selectedMemberId} onChange={(event) => setSelectedMemberId(event.target.value)}><option value="">Choose member</option>{communityMembers.filter((member) => member.id !== user.id).map((member) => <option key={member.id} value={member.id}>{member.username}</option>)}</select></label><button className="outline-button" disabled={!selectedMemberId} onClick={async () => { try { await api(`/api/v1/communities/${selectedCommunityId}/ownership`, { method: 'POST', body: JSON.stringify({ new_owner_id: selectedMemberId, confirm: true }) }); setCommunities((current) => current.map((item) => item.id === selectedCommunityId ? { ...item, owner_id: selectedMemberId } : item)) } catch (cause) { setError(cause instanceof Error ? cause.message : 'Could not transfer ownership') } }}>Transfer ownership</button><button className="danger-button" onClick={async () => { if (!window.confirm(`Permanently delete ${communityEditName}?`)) return; try { await api(`/api/v1/communities/${selectedCommunityId}`, { method: 'DELETE', body: JSON.stringify({ confirm: true }) }); const remaining = communities.filter((item) => item.id !== selectedCommunityId); setCommunities(remaining); setSelectedCommunityId(remaining[0]?.id ?? null); setChannels([]); setActiveId(null); setShowCommunityManager(false) } catch (cause) { setError(cause instanceof Error ? cause.message : 'Could not delete community') } }}>Delete community</button></section>}
    </section></div>}
    {showChannelComposer && <div className="modal-backdrop"><section className="dialog" role="dialog" aria-modal="true" aria-labelledby="channel-title"><button className="dialog-close" onClick={() => setShowChannelComposer(false)} aria-label="Close">×</button><p className="eyebrow">NEW TEXT CHANNEL</p><h2 id="channel-title">Create a channel</h2><form className="stack-form" onSubmit={createChannel}><label>Channel name<input autoFocus required maxLength={64} value={channelName} onChange={(event) => setChannelName(event.target.value)} placeholder="general" /></label><label>Category<select value={channelCategoryId} onChange={(event) => setChannelCategoryId(event.target.value)}><option value="">Uncategorized</option>{categories.map((category) => <option key={category.id} value={category.id}>{category.name}</option>)}</select></label><button className="primary-button">Create channel</button></form></section></div>}
  </main>
}
