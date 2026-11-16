import AxeBuilder from '@axe-core/playwright'
import { expect, test } from '@playwright/test'

test.beforeEach(async ({ page }) => {
  await page.route('**/api/v1/**', async (route) => {
    const path = new URL(route.request().url()).pathname
    if (route.request().method() === 'PUT' && path === '/api/v1/admin/users/user-2/suspension') {
      await route.fulfill({ json: { user: { id: 'user-2', username: 'bobby', created_at: '2026-11-14T00:00:00Z', is_instance_admin: false, deactivated_at: null, suspended_at: '2026-11-15T00:00:00Z', deleted_at: null } } })
      return
    }
    const bodies: Record<string, unknown> = {
      '/api/v1/auth/me': { user: { id: 'user-1', username: 'alice', created_at: '2026-11-15T00:00:00Z' } },
      '/api/v1/auth/profile': { profile: { id: 'user-1', username: 'alice', display_name: 'Alice', bio: '', updated_at: '2026-11-15T00:00:00Z' } },
      '/api/v1/conversations': { conversations: [] },
      '/api/v1/communities': { communities: [] },
      '/api/v1/admin/users': { users: [
        { id: 'user-1', username: 'alice', created_at: '2026-11-15T00:00:00Z', is_instance_admin: true, deactivated_at: null, suspended_at: null, deleted_at: null },
        { id: 'user-2', username: 'bobby', created_at: '2026-11-14T00:00:00Z', is_instance_admin: false, deactivated_at: null, suspended_at: null, deleted_at: null },
      ] },
    }
    await route.fulfill({ json: bodies[path] ?? {} })
  })
  await page.routeWebSocket(/\/api\/v1\/realtime/, (socket) => {
    socket.onMessage(() => {})
  })
})

test('core workspace and conversation dialog pass axe and keyboard focus checks', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 })
  await page.goto('/')
  await expect(page.getByRole('heading', { name: 'Messages', exact: true })).toBeVisible()
  await expect(page.getByRole('main')).toBeVisible()
  await expect(page.getByRole('complementary')).toHaveCount(2)

  const workspaceScan = await new AxeBuilder({ page })
    .withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa'])
    .analyze()
  expect(workspaceScan.violations.map(({ id, nodes }) => ({
    id,
    targets: nodes.map(({ target }) => target.join(' ')),
  }))).toEqual([])

  const trigger = page.getByRole('button', { name: 'New conversation' }).first()
  await trigger.focus()
  await page.keyboard.press('Control+k')
  const dialog = page.getByRole('dialog')
  await expect(dialog).toBeVisible()
  const username = dialog.getByLabel('Username')
  await expect(username).toBeFocused()

  const close = dialog.getByRole('button', { name: 'Close' })
  await close.focus()
  await page.keyboard.press('Shift+Tab')
  await expect(dialog.getByRole('button', { name: 'Continue' })).toBeFocused()
  await page.keyboard.press('Tab')
  await expect(close).toBeFocused()

  const dialogScan = await new AxeBuilder({ page })
    .withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa'])
    .analyze()
  expect(dialogScan.violations).toEqual([])

  await page.keyboard.press('Escape')
  await expect(dialog).not.toBeVisible()
  const focusAfterClose = await page.evaluate(() => ({
    active: document.activeElement?.outerHTML,
    visible: document.activeElement instanceof HTMLElement && Boolean(document.activeElement.offsetWidth),
  }))
  expect(focusAfterClose, 'focus should return to the dialog opener').toMatchObject({ active: await trigger.evaluate((element) => element.outerHTML), visible: true })
})

test('messaging workspace fits a narrow viewport without horizontal overflow', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 })
  await page.goto('/')
  await expect(page.getByRole('heading', { name: 'Messages', exact: true })).toBeVisible()
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true)
})

test('keyboard reaches workspace navigation, community join, settings, and admin controls', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 })
  await page.goto('/')
  await expect(page.getByRole('heading', { name: 'Messages', exact: true })).toBeVisible()

  await page.keyboard.press('Tab')
  await expect(page.getByRole('button', { name: 'Sign out alice' })).toBeFocused()
  await page.keyboard.press('Tab')
  await expect(page.getByRole('button', { name: 'New conversation' }).first()).toBeFocused()
  await page.keyboard.press('Tab')
  await expect(page.getByRole('button', { name: 'Create community' })).toBeFocused()
  await page.keyboard.press('Tab')
  const joinCommunity = page.getByRole('button', { name: 'Join community' })
  await expect(joinCommunity).toBeFocused()
  await page.keyboard.press('Enter')
  const joinDialog = page.getByRole('dialog')
  await expect(joinDialog.getByLabel('Invite code')).toBeFocused()
  await page.keyboard.press('Escape')
  await expect(joinDialog).not.toBeVisible()
  await expect(joinCommunity).toBeFocused()

  const appearance = page.getByText('Appearance', { exact: true })
  await appearance.focus()
  await page.keyboard.press('Enter')
  await expect(page.getByLabel('Theme')).toBeVisible()

  const adminSection = page.getByText('Instance administration', { exact: true })
  await adminSection.focus()
  await page.keyboard.press('Enter')
  const suspendBobby = page.getByRole('button', { name: 'Suspend bobby' })
  await expect(suspendBobby).toBeVisible()
  await suspendBobby.focus()
  await page.keyboard.press('Enter')
  await expect(page.getByRole('button', { name: 'Restore bobby' })).toBeVisible()
})

test('browser workflow creates a conversation, sends a message, and creates a community channel', async ({ page }) => {
  const now = '2026-11-15T00:00:00Z'
  const conversationId = '019a0000-0000-7000-8000-000000000001'
  const communityId = '019a0000-0000-7000-8000-000000000002'
  const channelId = '019a0000-0000-7000-8000-000000000003'
  const channelConversationId = '019a0000-0000-7000-8000-000000000004'
  const conversations: Array<{ id: string; kind: 'direct' | 'group'; created_by: string; created_at: string; members: string[] }> = []
  const messages: Array<{ id: string; conversation_id: string; sender_id: string; sequence: number; client_message_id: string; content: string; created_at: string }> = []
  const communities: Array<{ id: string; owner_id: string; name: string; description: string; created_at: string; member_count: number }> = []
  const channels: Array<{ id: string; community_id: string; conversation_id: string; category_id: string | null; name: string; description: string; position: number; read_only: boolean; created_at: string }> = []

  await page.route('**/api/v1/**', async (route) => {
    const { pathname } = new URL(route.request().url())
    const method = route.request().method()
    if (pathname === '/api/v1/conversations' && method === 'GET') {
      await route.fulfill({ json: { conversations } })
    } else if (pathname === '/api/v1/conversations' && method === 'POST') {
      const conversation = { id: conversationId, kind: 'direct' as const, created_by: 'user-1', created_at: now, members: ['user-1', 'user-2'] }
      conversations.unshift(conversation)
      await route.fulfill({ status: 201, json: { conversation } })
    } else if (pathname === `/api/v1/conversations/${conversationId}/messages` && method === 'GET') {
      await route.fulfill({ json: { messages: [...messages], next_before: null } })
    } else if (pathname === `/api/v1/conversations/${conversationId}/messages` && method === 'POST') {
      const body = route.request().postDataJSON() as { client_message_id: string; content: string }
      const message = { id: '019a0000-0000-7000-8000-000000000005', conversation_id: conversationId, sender_id: 'user-1', sequence: 1, client_message_id: body.client_message_id, content: body.content, created_at: now }
      messages.push(message)
      await route.fulfill({ status: 201, json: { message } })
    } else if (pathname === '/api/v1/communities' && method === 'GET') {
      await route.fulfill({ json: { communities } })
    } else if (pathname === '/api/v1/communities' && method === 'POST') {
      const body = route.request().postDataJSON() as { name: string }
      const community = { id: communityId, owner_id: 'user-1', name: body.name, description: '', created_at: now, member_count: 1 }
      communities.push(community)
      await route.fulfill({ status: 201, json: { community } })
    } else if (pathname === `/api/v1/communities/${communityId}/channels` && method === 'GET') {
      await route.fulfill({ json: { channels } })
    } else if (pathname === `/api/v1/communities/${communityId}/categories` && method === 'GET') {
      await route.fulfill({ json: { categories: [] } })
    } else if (pathname === `/api/v1/communities/${communityId}/channels` && method === 'POST') {
      const body = route.request().postDataJSON() as { name: string }
      const channel = { id: channelId, community_id: communityId, conversation_id: channelConversationId, category_id: null, name: body.name, description: '', position: 0, read_only: false, created_at: now }
      channels.push(channel)
      await route.fulfill({ status: 201, json: { channel } })
    } else if (/^\/api\/v1\/conversations\/[^/]+\/members$/.test(pathname) && method === 'GET') {
      await route.fulfill({ json: { members: [{ id: 'user-1', username: 'alice' }, { id: 'user-2', username: 'bobby' }] } })
    } else if (/^\/api\/v1\/conversations\/[^/]+\/(read-cursors|delivery-cursors)$/.test(pathname) && method === 'GET') {
      await route.fulfill({ json: { cursors: [] } })
    } else if (/^\/api\/v1\/conversations\/[^/]+\/read-cursor$/.test(pathname) && method === 'PUT') {
      await route.fulfill({ json: { cursor: { user_id: 'user-1', sequence: 1 } } })
    } else if (/^\/api\/v1\/conversations\/[^/]+\/messages$/.test(pathname) && method === 'GET') {
      await route.fulfill({ json: { messages: [], next_before: null } })
    } else {
      await route.fallback()
    }
  })

  await page.goto('/')
  await expect(page.getByRole('heading', { name: 'Messages', exact: true })).toBeVisible()
  await page.getByRole('button', { name: 'New conversation' }).first().click()
  const conversationDialog = page.getByRole('dialog')
  await conversationDialog.getByLabel('Username').fill('bobby')
  await conversationDialog.getByRole('button', { name: 'Continue' }).click()
  await expect(conversationDialog).not.toBeVisible()

  const composer = page.getByRole('textbox', { name: 'Write a message' })
  await composer.fill('Hello from the browser journey')
  await composer.press('Enter')
  await expect(page.getByText('Hello from the browser journey')).toBeVisible()

  // Message actions follow the pointer: shown on hover, hidden again once it leaves, even after a click.
  const toolbar = page.getByRole('group', { name: 'Message actions' }).last()
  await page.getByText('Hello from the browser journey').click()
  await page.mouse.move(5, 5)
  await expect(toolbar).toHaveCSS('opacity', '0')
  await page.getByText('Hello from the browser journey').hover()
  await expect(toolbar).toHaveCSS('opacity', '1')
  const messageLog = page.getByRole('log')
  await expect(messageLog).toHaveAttribute('aria-live', 'polite')
  await expect(messageLog).toHaveAttribute('aria-label', /Messages in/)
  const messageScan = await new AxeBuilder({ page })
    .withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa'])
    .analyze()
  expect(messageScan.violations.map(({ id, nodes }) => ({ id, targets: nodes.map(({ target }) => target.join(' ')) }))).toEqual([])

  await page.getByRole('button', { name: 'Create community' }).click()
  const communityDialog = page.getByRole('dialog')
  await communityDialog.getByLabel('Community name').fill('RustChat Study')
  await communityDialog.getByRole('button', { name: 'Create community' }).click()
  await expect(page.getByRole('button', { name: /RustChat Study/ })).toBeVisible()
  await page.getByRole('button', { name: 'Create channel' }).click()
  const channelDialog = page.getByRole('dialog')
  await channelDialog.getByLabel('Channel name').fill('general')
  await channelDialog.getByRole('button', { name: 'Create channel' }).click()
  await expect(page.getByRole('button', { name: /general/ })).toBeVisible()
  const channelScan = await new AxeBuilder({ page })
    .withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa'])
    .analyze()
  expect(channelScan.violations.map(({ id, nodes }) => ({ id, targets: nodes.map(({ target }) => target.join(' ')) }))).toEqual([])
})

test('dark theme meets WCAG 2.1 A/AA contrast checks', async ({ page }) => {
  await page.addInitScript(() => localStorage.setItem('rustchat-theme', 'dark'))
  await page.goto('/')
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark')
  const scan = await new AxeBuilder({ page })
    .withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa'])
    .analyze()
  expect(scan.violations.map(({ id, nodes }) => ({
    id,
    targets: nodes.map(({ target }) => target.join(' ')),
  }))).toEqual([])
})
