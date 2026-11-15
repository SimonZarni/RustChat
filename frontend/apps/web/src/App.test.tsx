import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import App from './App'

describe('App', () => {
  afterEach(() => { cleanup(); window.history.replaceState({}, '', '/'); localStorage.removeItem('rustchat-theme'); document.documentElement.dataset.theme = 'light'; vi.unstubAllGlobals() })

  it('shows the sign-in form when there is no active session', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response(
      JSON.stringify({ error: { code: 'UNAUTHENTICATED', message: 'Authentication is required.' } }),
      { status: 401, headers: { 'Content-Type': 'application/json' } },
    )))

    render(<App />)

    expect(await screen.findByRole('heading', { name: 'Welcome back' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Sign in' })).toBeInTheDocument()
  })

  it('requests a password reset without revealing account eligibility', async () => {
    const fetchMock = vi.fn(async (input: RequestInfo | URL) => {
      if (String(input) === '/api/v1/auth/me') return new Response('', { status: 401 })
      if (String(input) === '/api/v1/auth/password-reset/request') return new Response(null, { status: 202 })
      return new Response('{}', { status: 200 })
    })
    vi.stubGlobal('fetch', fetchMock)
    render(<App />)

    fireEvent.click(await screen.findByRole('button', { name: 'Forgot password?' }))
    fireEvent.change(screen.getByLabelText('Email'), { target: { value: 'alice@example.test' } })
    fireEvent.click(screen.getByRole('button', { name: 'Send reset link' }))

    expect(await screen.findByRole('status')).toHaveTextContent('If an eligible account matches that address')
    expect(fetchMock).toHaveBeenCalledWith('/api/v1/auth/password-reset/request', expect.objectContaining({
      method: 'POST',
      body: JSON.stringify({ email: 'alice@example.test' }),
    }))
  })

  it('completes a password reset using the email link token', async () => {
    window.history.replaceState({}, '', '/reset-password?token=reset-token')
    const fetchMock = vi.fn(async (input: RequestInfo | URL) => {
      if (String(input) === '/api/v1/auth/me') return new Response('', { status: 401 })
      if (String(input) === '/api/v1/auth/password-reset/complete') return new Response(null, { status: 204 })
      return new Response('{}', { status: 200 })
    })
    vi.stubGlobal('fetch', fetchMock)
    render(<App />)

    fireEvent.change(await screen.findByLabelText('New password'), { target: { value: 'a secure replacement password' } })
    fireEvent.click(screen.getByRole('button', { name: 'Update password' }))

    expect(await screen.findByRole('status')).toHaveTextContent('Password updated. Sign in with your new password.')
    expect(fetchMock).toHaveBeenCalledWith('/api/v1/auth/password-reset/complete', expect.objectContaining({
      method: 'POST',
      body: JSON.stringify({ token: 'reset-token', new_password: 'a secure replacement password' }),
    }))
  })

  it('loads and saves the signed-in user profile', async () => {
    class FakeWebSocket {
      static OPEN = 1
      static latest: FakeWebSocket | null = null
      readyState = 1
      onopen: (() => void) | null = null
      onclose: (() => void) | null = null
      onerror: (() => void) | null = null
      onmessage: ((event: MessageEvent) => void) | null = null
      constructor() { FakeWebSocket.latest = this }
      send() {}
      close() { this.onclose?.() }
    }
    vi.stubGlobal('WebSocket', FakeWebSocket)
    const profile = { id: 'user-1', username: 'alice', display_name: 'Alice', bio: '', updated_at: '2026-11-15T00:00:00Z' }
    const privacy = { discoverable: true, profile_visibility: 'everyone', presence_visible: true }
    const community = { id: 'community-1', owner_id: 'user-1', name: 'Study Room', description: '', created_at: '2026-11-15T00:00:00Z', member_count: 1 }
    const category = { id: 'category-1', community_id: 'community-1', name: 'Projects', position: 0 }
    const createdRole = { id: 'role-2', community_id: 'community-1', name: 'Helper', position: 1, permissions: 193, is_default: false }
    const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (path === '/api/v1/auth/me') return new Response(JSON.stringify({ user: { id: 'user-1', username: 'alice', created_at: '2026-11-15T00:00:00Z' } }), { status: 200 })
      if (path === '/api/v1/auth/login' && init?.method === 'POST') return new Response(JSON.stringify({ user: { id: 'user-1', username: 'alice', created_at: '2026-11-15T00:00:00Z' } }), { status: 200 })
      if (path === '/api/v1/auth/sessions') return new Response(JSON.stringify({ sessions: [{ id: 'session-1', created_at: '2026-11-14T00:00:00Z', expires_at: '2026-12-14T00:00:00Z', current: true }, { id: 'session-2', created_at: '2026-11-13T00:00:00Z', expires_at: '2026-12-13T00:00:00Z', current: false }] }), { status: 200 })
      if (path === '/api/v1/auth/password' && init?.method === 'POST') return new Response(null, { status: 204 })
      if ((path === '/api/v1/auth/deactivate' || path === '/api/v1/auth/delete') && init?.method === 'POST') return new Response(null, { status: 204 })
      if (path === '/api/v1/auth/sessions/session-2' && init?.method === 'DELETE') return new Response(null, { status: 204 })
      if (path === '/api/v1/auth/privacy' && init?.method === 'PATCH') {
        Object.assign(privacy, JSON.parse(String(init.body)))
        return new Response(JSON.stringify({ privacy }), { status: 200 })
      }
      if (path === '/api/v1/auth/privacy') return new Response(JSON.stringify({ privacy }), { status: 200 })
      if (path === '/api/v1/auth/profile' && init?.method === 'PATCH') {
        Object.assign(profile, JSON.parse(String(init.body)))
        return new Response(JSON.stringify({ profile }), { status: 200 })
      }
      if (path === '/api/v1/auth/username' && init?.method === 'PATCH') {
        const user = { id: 'user-1', username: JSON.parse(String(init.body)).username, created_at: '2026-11-15T00:00:00Z' }
        profile.username = user.username
        return new Response(JSON.stringify({ user }), { status: 200 })
      }
      if (path.startsWith('/api/v1/users/') && path.endsWith('/profile')) {
        const viewedUsername = decodeURIComponent(path.split('/').at(-2) ?? '')
        return new Response(JSON.stringify({ profile: { ...profile, username: viewedUsername, display_name: 'Viewed user' } }), { status: 200 })
      }
      if (path === '/api/v1/auth/profile') return new Response(JSON.stringify({ profile }), { status: 200 })
      if (path.startsWith('/api/v1/conversations')) return new Response(JSON.stringify({ conversations: [] }), { status: 200 })
      if (path === '/api/v1/communities') return new Response(JSON.stringify({ communities: [community] }), { status: 200 })
      if (path === '/api/v1/communities/community-1/invites' && init?.method === 'POST') return new Response(JSON.stringify({ invite_code: 'a'.repeat(64) }), { status: 201 })
      if (path === '/api/v1/communities/community-1/invites' && init?.method === 'DELETE') return new Response(null, { status: 204 })
      if (path === '/api/v1/communities/community-1/invites/invite-1' && init?.method === 'DELETE') return new Response(null, { status: 204 })
      if (path === '/api/v1/communities/community-1/invites') return new Response(JSON.stringify({ invites: [{ id: 'invite-1', created_at: '2026-11-14T00:00:00Z', expires_at: null, max_uses: 10, use_count: 2, revoked_at: null }] }), { status: 200 })
      if (path === '/api/v1/communities/community-1/channels' && init?.method === 'POST') return new Response(JSON.stringify({ channel: { id: 'channel-1', community_id: 'community-1', conversation_id: 'channel-conversation-1', category_id: JSON.parse(String(init.body)).category_id, name: 'planning', description: '', position: 0, read_only: false, created_at: '2026-11-15T00:00:00Z' } }), { status: 201 })
      if (path === '/api/v1/communities/community-1/channels/channel-1' && init?.method === 'PATCH') return new Response(JSON.stringify({ channel: { id: 'channel-1', community_id: 'community-1', conversation_id: 'channel-conversation-1', ...JSON.parse(String(init.body)), created_at: '2026-11-15T00:00:00Z' } }), { status: 200 })
      if (path === '/api/v1/communities/community-1/channels') return new Response(JSON.stringify({ channels: [] }), { status: 200 })
      if (path === '/api/v1/communities/community-1/categories' && init?.method === 'POST') return new Response(JSON.stringify({ category }), { status: 201 })
      if (path === '/api/v1/communities/community-1/categories') return new Response(JSON.stringify({ categories: [] }), { status: 200 })
      if (path === '/api/v1/communities/community-1/roles' && init?.method === 'POST') return new Response(JSON.stringify({ role: createdRole }), { status: 201 })
      if (path === '/api/v1/communities/community-1/roles') return new Response(JSON.stringify({ roles: [{ id: 'role-default', community_id: 'community-1', name: 'everyone', position: 0, permissions: 193, is_default: true }] }), { status: 200 })
      if (path === '/api/v1/communities/community-1/members') return new Response(JSON.stringify({ members: [{ id: 'user-1', username: 'alice', created_at: '2026-11-15T00:00:00Z' }] }), { status: 200 })
      return new Response(JSON.stringify({}), { status: 200 })
    })
    vi.stubGlobal('fetch', fetchMock)

    render(<App />)

    const communityButton = await screen.findByRole('button', { name: /Study Room/ })
    fireEvent.click(communityButton)
    fireEvent.click(screen.getByText('Appearance', { selector: 'summary' }))
    const appearanceSettings = screen.getByText('Appearance', { selector: 'summary' }).closest('details')!
    fireEvent.change(within(appearanceSettings).getByLabelText('Theme'), { target: { value: 'dark' } })
    await waitFor(() => expect(document.documentElement.dataset.theme).toBe('dark'))
    expect(localStorage.getItem('rustchat-theme')).toBe('dark')
    fireEvent.click(await screen.findByRole('button', { name: 'Create invite' }))
    expect(await screen.findByText(`Invite code: ${'a'.repeat(64)}`)).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Revoke' }))
    await waitFor(() => expect(fetchMock).toHaveBeenCalledWith('/api/v1/communities/community-1/invites', expect.objectContaining({
      method: 'DELETE',
      body: JSON.stringify({ invite_code: 'a'.repeat(64) }),
    })))
    await waitFor(() => expect(screen.queryByText(`Invite code: ${'a'.repeat(64)}`)).not.toBeInTheDocument())
    const channelListLoads = () => fetchMock.mock.calls.filter(([input]) => String(input) === '/api/v1/communities/community-1/channels').length
    await waitFor(() => expect(channelListLoads()).toBeGreaterThan(0))
    const channelLoadsBeforeEvent = channelListLoads()
    FakeWebSocket.latest?.onmessage?.(new MessageEvent('message', { data: JSON.stringify({
      type: 'channels.updated',
      payload: { community_id: 'community-1' },
    }) }))
    await waitFor(() => expect(channelListLoads()).toBeGreaterThan(channelLoadsBeforeEvent))

    fireEvent.click(await screen.findByText('Edit', { selector: 'summary' }))
    const profileSettings = screen.getByText('Edit', { selector: 'summary' }).closest('details')!
    fireEvent.change(await within(profileSettings).findByLabelText('Username'), { target: { value: 'alice_new' } })
    fireEvent.click(within(profileSettings).getByRole('button', { name: 'Update username' }))
    await waitFor(() => expect(fetchMock).toHaveBeenCalledWith('/api/v1/auth/username', expect.objectContaining({ method: 'PATCH' })))
    expect(await screen.findByText('@alice_new')).toBeInTheDocument()
    fireEvent.change(await screen.findByLabelText('Display name'), { target: { value: 'Alice Rivers' } })
    fireEvent.change(screen.getByLabelText('About you'), { target: { value: 'Here to chat.' } })
    fireEvent.click(screen.getByRole('button', { name: 'Save profile' }))

    await waitFor(() => expect(fetchMock).toHaveBeenCalledWith('/api/v1/auth/profile', expect.objectContaining({ method: 'PATCH' })))
    expect(await screen.findByText('Alice Rivers')).toBeInTheDocument()

    fireEvent.click(screen.getByText('Security', { selector: 'summary' }))
    const securitySettings = screen.getByText('Security', { selector: 'summary' }).closest('details')!
    expect(await within(securitySettings).findByText('This session')).toBeInTheDocument()
    expect(await within(securitySettings).findByText('Signed in')).toBeInTheDocument()
    fireEvent.change(within(securitySettings).getByLabelText('Current password'), { target: { value: 'old-password' } })
    fireEvent.change(within(securitySettings).getByLabelText('New password'), { target: { value: 'a-new-password-123' } })
    fireEvent.click(within(securitySettings).getByRole('button', { name: 'Change password' }))
    await waitFor(() => expect(fetchMock).toHaveBeenCalledWith('/api/v1/auth/password', expect.objectContaining({ method: 'POST' })))
    const passwordRequest = fetchMock.mock.calls.find(([input]) => String(input) === '/api/v1/auth/password')
    expect(JSON.parse(String(passwordRequest?.[1]?.body))).toEqual({ current_password: 'old-password', new_password: 'a-new-password-123' })
    expect(await screen.findByRole('status')).toHaveTextContent('Password updated')
    fireEvent.click(within(securitySettings).getByRole('button', { name: 'Revoke' }))
    await waitFor(() => expect(fetchMock).toHaveBeenCalledWith('/api/v1/auth/sessions/session-2', expect.objectContaining({ method: 'DELETE' })))
    await waitFor(() => expect(screen.queryByText('Signed in')).not.toBeInTheDocument())

    fireEvent.click(screen.getByText('Privacy', { selector: 'summary' }))
    fireEvent.click(await screen.findByLabelText('Allow people to find me by username'))
    fireEvent.change(screen.getByLabelText('Who can view my profile'), { target: { value: 'conversation_members' } })
    fireEvent.click(screen.getByRole('button', { name: 'Save privacy' }))
    await waitFor(() => expect(fetchMock).toHaveBeenCalledWith('/api/v1/auth/privacy', expect.objectContaining({ method: 'PATCH' })))
    const privacyRequest = fetchMock.mock.calls.find(([input, init]) => String(input) === '/api/v1/auth/privacy' && init?.method === 'PATCH')
    expect(JSON.parse(String(privacyRequest?.[1]?.body))).toEqual({ discoverable: false, profile_visibility: 'conversation_members', presence_visible: true })

    const focusBeforeShortcut = document.activeElement
    fireEvent.keyDown(window, { key: 'k', ctrlKey: true })
    const conversationDialog = await screen.findByRole('dialog')
    const usernameField = within(conversationDialog).getByLabelText('Username')
    expect(document.activeElement).toBe(usernameField)
    const closeDialogButton = within(conversationDialog).getByRole('button', { name: 'Close' })
    closeDialogButton.focus()
    fireEvent.keyDown(closeDialogButton, { key: 'Tab', shiftKey: true })
    expect(document.activeElement).toBe(within(conversationDialog).getByRole('button', { name: 'Continue' }))
    fireEvent.keyDown(document.activeElement!, { key: 'Tab' })
    expect(document.activeElement).toBe(within(conversationDialog).getByRole('button', { name: 'Close' }))
    fireEvent.change(usernameField, { target: { value: 'bobby' } })
    fireEvent.click(within(conversationDialog).getByRole('button', { name: 'View profile' }))
    expect(await within(conversationDialog).findByText('Viewed user')).toBeInTheDocument()
    fireEvent.keyDown(window, { key: 'Escape' })
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument())
    expect(document.activeElement).toBe(focusBeforeShortcut)

    fireEvent.click(await screen.findByRole('button', { name: 'Manage' }))
    expect(await screen.findByRole('heading', { name: 'Manage Study Room' })).toBeInTheDocument()
    expect(screen.getByText(/Active · 2\/10 uses/)).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Revoke' }))
    await waitFor(() => expect(fetchMock).toHaveBeenCalledWith('/api/v1/communities/community-1/invites/invite-1', expect.objectContaining({ method: 'DELETE' })))
    expect(await screen.findByText(/Revoked · 2\/10 uses/)).toBeInTheDocument()
    fireEvent.change(screen.getByLabelText('New role'), { target: { value: 'Helper' } })
    fireEvent.click(screen.getByRole('button', { name: 'Create role' }))
    await waitFor(() => expect(fetchMock).toHaveBeenCalledWith('/api/v1/communities/community-1/roles', expect.objectContaining({ method: 'POST' })))
    expect((await screen.findAllByText('Helper')).length).toBeGreaterThan(0)
    fireEvent.change(screen.getByLabelText('New category'), { target: { value: 'Projects' } })
    fireEvent.click(screen.getByRole('button', { name: 'Create category' }))
    await waitFor(() => expect(fetchMock).toHaveBeenCalledWith('/api/v1/communities/community-1/categories', expect.objectContaining({ method: 'POST' })))
    fireEvent.click(screen.getByRole('button', { name: 'Close community management' }))
    fireEvent.click(screen.getByRole('button', { name: 'Create channel' }))
    fireEvent.change(await screen.findByLabelText('Channel name'), { target: { value: 'planning' } })
    fireEvent.change(screen.getByLabelText('Category'), { target: { value: 'category-1' } })
    fireEvent.click(screen.getAllByRole('button', { name: 'Create channel' }).at(-1)!)
    await waitFor(() => expect(fetchMock).toHaveBeenCalledWith('/api/v1/communities/community-1/channels', expect.objectContaining({ method: 'POST' })))
    const channelRequest = fetchMock.mock.calls.find(([input, init]) => String(input) === '/api/v1/communities/community-1/channels' && init?.method === 'POST')
    expect(JSON.parse(String(channelRequest?.[1]?.body))).toEqual({ name: 'planning', category_id: 'category-1' })
    fireEvent.click(screen.getByRole('button', { name: 'Manage' }))
    fireEvent.change(await screen.findByLabelText('Category for planning'), { target: { value: '' } })
    await waitFor(() => expect(fetchMock).toHaveBeenCalledWith('/api/v1/communities/community-1/channels/channel-1', expect.objectContaining({ method: 'PATCH' })))
    const moveRequest = fetchMock.mock.calls.find(([input, init]) => String(input).endsWith('/channels/channel-1') && init?.method === 'PATCH')
    expect(JSON.parse(String(moveRequest?.[1]?.body))).toMatchObject({ category_id: null })

    const accountSettings = screen.getByText('Account actions', { selector: 'summary' }).closest('details')!
    fireEvent.click(within(accountSettings).getByText('Account actions', { selector: 'summary' }))
    fireEvent.change(within(accountSettings).getByLabelText('Current password'), { target: { value: 'old-password' } })
    fireEvent.click(within(accountSettings).getByRole('button', { name: 'Deactivate account' }))
    await waitFor(() => expect(fetchMock).toHaveBeenCalledWith('/api/v1/auth/deactivate', expect.objectContaining({ method: 'POST' })))
    const deactivateRequest = fetchMock.mock.calls.find(([input]) => String(input) === '/api/v1/auth/deactivate')
    expect(JSON.parse(String(deactivateRequest?.[1]?.body))).toEqual({ current_password: 'old-password' })
    expect(await screen.findByRole('heading', { name: 'Welcome back' })).toBeInTheDocument()

    fireEvent.change(screen.getByLabelText('Username'), { target: { value: 'alice' } })
    fireEvent.change(screen.getByLabelText('Password'), { target: { value: 'old-password' } })
    fireEvent.click(screen.getByRole('button', { name: 'Sign in' }))
    await screen.findByRole('heading', { name: 'Messages' })
    const reopenedAccount = screen.getByText('Account actions', { selector: 'summary' }).closest('details')!
    fireEvent.click(within(reopenedAccount).getByText('Account actions', { selector: 'summary' }))
    fireEvent.change(within(reopenedAccount).getByLabelText('Current password'), { target: { value: 'old-password' } })
    const deleteButton = within(reopenedAccount).getByRole('button', { name: 'Delete account and scrub profile' })
    expect(deleteButton).toBeDisabled()
    fireEvent.change(within(reopenedAccount).getByLabelText('Type DELETE to permanently delete your account'), { target: { value: 'DELETE' } })
    fireEvent.click(deleteButton)
    await waitFor(() => expect(fetchMock).toHaveBeenCalledWith('/api/v1/auth/delete', expect.objectContaining({ method: 'POST' })))
    const deleteRequest = fetchMock.mock.calls.find(([input]) => String(input) === '/api/v1/auth/delete')
    expect(JSON.parse(String(deleteRequest?.[1]?.body))).toEqual({ current_password: 'old-password', confirm: true })
  }, 15000)

  it('loads reactions and lets a member react and reply to a message', async () => {
    class FakeWebSocket {
      static OPEN = 1
      readyState = 1
      onopen: (() => void) | null = null
      onclose: (() => void) | null = null
      onerror: (() => void) | null = null
      onmessage: ((event: MessageEvent) => void) | null = null
      send() {}
      close() { this.onclose?.() }
    }
    vi.stubGlobal('WebSocket', FakeWebSocket)
    const conversation = { id: 'conversation-1', kind: 'direct', created_by: 'user-1', created_at: '2026-11-15T00:00:00Z', members: ['user-1', 'user-2'] }
    const message = { id: 'message-1', conversation_id: 'conversation-1', sender_id: 'user-1', sequence: 1, client_message_id: 'client-1', mentioned_user_ids: ['user-2'], content: 'Nice **work** @bobby\n- [Rust](https://www.rust-lang.org)\n`inline()`\n```rust\nfn main() {}\n```', created_at: '2026-11-15T00:00:00Z' }
    const profile = { id: 'user-1', username: 'alice', display_name: 'Alice', bio: '', updated_at: '2026-11-15T00:00:00Z' }
    const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (path === '/api/v1/auth/me') return new Response(JSON.stringify({ user: { id: 'user-1', username: 'alice', created_at: profile.updated_at } }), { status: 200 })
      if (path === '/api/v1/auth/profile') return new Response(JSON.stringify({ profile }), { status: 200 })
      if (path === '/api/v1/conversations?limit=100') return new Response(JSON.stringify({ conversations: [conversation] }), { status: 200 })
      if (path === '/api/v1/communities') return new Response(JSON.stringify({ communities: [] }), { status: 200 })
      if (path === '/api/v1/communities/community-1/channels') return new Response(JSON.stringify({ channels: [] }), { status: 200 })
      if (path === '/api/v1/communities/community-1/categories') return new Response(JSON.stringify({ categories: [] }), { status: 200 })
      if (path.endsWith('/messages?limit=50')) return new Response(JSON.stringify({ messages: [message], next_before: null }), { status: 200 })
      if (path.endsWith('/members')) return new Response(JSON.stringify({ members: [{ id: 'user-1', username: 'alice' }, { id: 'user-2', username: 'bobby' }] }), { status: 200 })
      if (path.endsWith('/read-cursors')) return new Response(JSON.stringify({ cursors: [] }), { status: 200 })
      if (path.endsWith('/delivery-cursors')) return new Response(JSON.stringify({ cursors: [{ user_id: 'user-2', sequence: 1 }] }), { status: 200 })
      if (path.includes('/reactions?message_ids=')) return new Response(JSON.stringify({ reactions: [] }), { status: 200 })
      if (path.endsWith('/read-cursor')) return new Response(JSON.stringify({ cursor: { user_id: 'user-1', sequence: 1 } }), { status: 200 })
      if (path.endsWith('/messages') && init?.method === 'POST') return new Response(JSON.stringify({ message: { ...message, id: 'message-2', sequence: 2, client_message_id: 'client-2', reply_to_message_id: 'message-1', content: 'Thanks for the note.' } }), { status: 201 })
      if (path.endsWith('/messages/message-1/reactions') && init?.method === 'PUT') return new Response(JSON.stringify({ sequence: 1, changed: true, reactions: [{ message_id: 'message-1', emoji: '👍', count: 1, reacted_by_me: true }] }), { status: 200 })
      return new Response(JSON.stringify({}), { status: 200 })
    })
    vi.stubGlobal('fetch', fetchMock)

    render(<App />)

    expect(await screen.findByText('Delivered to 1')).toBeInTheDocument()
    expect(screen.getByRole('log')).toHaveAttribute('aria-live', 'polite')
    expect(await screen.findByText('@bobby')).toHaveClass('mention-highlight')
    expect(screen.getByText('work').tagName).toBe('STRONG')
    expect(screen.getByRole('link', { name: 'Rust' })).toHaveAttribute('href', 'https://www.rust-lang.org')
    expect(screen.getByRole('listitem')).toHaveTextContent('Rust')
    expect(screen.getByText('inline()').tagName).toBe('CODE')
    expect(screen.getByText('fn main() {}').tagName).toBe('CODE')
    const addReaction = await screen.findByRole('button', { name: 'Add 👍 reaction' })
    fireEvent.click(addReaction)
    expect(await screen.findByRole('button', { name: 'Remove 👍 reaction, 1 total' })).toHaveAttribute('aria-pressed', 'true')
    expect(fetchMock).toHaveBeenCalledWith('/api/v1/conversations/conversation-1/messages/message-1/reactions', expect.objectContaining({ method: 'PUT' }))
    fireEvent.click(screen.getByRole('button', { name: 'Reply' }))
    expect(await screen.findByText(/Replying to alice: Nice \*\*work\*\*/)).toBeInTheDocument()
    fireEvent.change(screen.getByRole('textbox', { name: 'Write a message' }), { target: { value: 'Thanks for the note.' } })
    fireEvent.click(screen.getByRole('button', { name: 'Send message' }))
    await waitFor(() => expect(fetchMock).toHaveBeenCalledWith('/api/v1/conversations/conversation-1/messages', expect.objectContaining({ method: 'POST' })))
    const sendRequest = fetchMock.mock.calls.find(([input, init]) => String(input).endsWith('/messages') && init?.method === 'POST')
    expect(JSON.parse(String(sendRequest?.[1]?.body))).toMatchObject({ reply_to_message_id: 'message-1', content: 'Thanks for the note.' })
  })

  it('refreshes durable read cursors after a websocket reconnect', async () => {
    class FakeWebSocket {
      static OPEN = 1
      static instances: FakeWebSocket[] = []
      readyState = 1
      onopen: (() => void) | null = null
      onclose: (() => void) | null = null
      onerror: (() => void) | null = null
      onmessage: ((event: MessageEvent) => void) | null = null
      constructor() {
        FakeWebSocket.instances.push(this)
        queueMicrotask(() => this.onopen?.())
      }
      send() {}
      close() { this.readyState = 3; this.onclose?.() }
    }
    vi.stubGlobal('WebSocket', FakeWebSocket)
    let durableReadSequence = 0
    const conversation = { id: 'conversation-1', kind: 'direct', created_by: 'user-1', created_at: '2026-11-15T00:00:00Z', members: ['user-1', 'user-2'] }
    const message = { id: 'message-1', conversation_id: 'conversation-1', sender_id: 'user-1', sequence: 1, client_message_id: 'client-1', content: 'Hello', created_at: '2026-11-15T00:00:00Z' }
    const fetchMock = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input)
      if (path === '/api/v1/auth/me') return new Response(JSON.stringify({ user: { id: 'user-1', username: 'alice', created_at: '2026-11-15T00:00:00Z' } }), { status: 200 })
      if (path === '/api/v1/auth/profile') return new Response(JSON.stringify({ profile: { id: 'user-1', username: 'alice', display_name: 'Alice', bio: '', updated_at: '2026-11-15T00:00:00Z' } }), { status: 200 })
      if (path === '/api/v1/conversations?limit=100') return new Response(JSON.stringify({ conversations: [conversation] }), { status: 200 })
      if (path === '/api/v1/communities') return new Response(JSON.stringify({ communities: [] }), { status: 200 })
      if (path.endsWith('/messages?limit=50') || path.endsWith('/messages?limit=100')) return new Response(JSON.stringify({ messages: [message], next_before: null }), { status: 200 })
      if (path.endsWith('/members')) return new Response(JSON.stringify({ members: [{ id: 'user-1', username: 'alice' }, { id: 'user-2', username: 'bobby' }] }), { status: 200 })
      if (path.endsWith('/read-cursors')) return new Response(JSON.stringify({ cursors: [{ user_id: 'user-2', sequence: durableReadSequence }] }), { status: 200 })
      if (path.endsWith('/delivery-cursors')) return new Response(JSON.stringify({ cursors: [] }), { status: 200 })
      if (path.endsWith('/read-cursor')) return new Response(JSON.stringify({ cursor: { user_id: 'user-1', sequence: 1 } }), { status: 200 })
      if (path.includes('/reactions?message_ids=')) return new Response(JSON.stringify({ reactions: [] }), { status: 200 })
      return new Response(JSON.stringify({}), { status: 200 })
    })
    vi.stubGlobal('fetch', fetchMock)

    render(<App />)
    expect(await screen.findByRole('heading', { name: 'Messages' })).toBeInTheDocument()
    await waitFor(() => expect(FakeWebSocket.instances.length).toBeGreaterThan(0))
    expect(screen.queryByText('Seen by 1')).not.toBeInTheDocument()

    durableReadSequence = 1
    FakeWebSocket.instances[0]!.close()
    await screen.findByText('Seen by 1', {}, { timeout: 5000 })
    expect(fetchMock.mock.calls.filter(([input]) => String(input).endsWith('/read-cursors')).length).toBeGreaterThanOrEqual(2)
  }, 12000)
})
