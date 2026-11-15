import axe from 'axe-core'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import App from './App'

describe('core accessibility', () => {
  afterEach(() => {
    cleanup()
    window.history.replaceState({}, '', '/')
    vi.unstubAllGlobals()
  })

  it('keeps the sign-in workflow free of WCAG 2.1 A/AA violations', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response(
      JSON.stringify({ error: { code: 'UNAUTHENTICATED', message: 'Authentication is required.' } }),
      { status: 401, headers: { 'Content-Type': 'application/json' } },
    )))

    render(<App />)
    expect(await screen.findByRole('heading', { name: 'Welcome back' })).toBeInTheDocument()

    const results = await axe.run(document.body, {
      runOnly: { type: 'tag', values: ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa'] },
    })
    expect(results.violations.map(({ id, help, nodes }) => ({
      id,
      help,
      elements: nodes.map(({ target }) => target),
    }))).toEqual([])
  })

  it('keeps the authenticated messaging shell free of WCAG 2.1 A/AA violations', async () => {
    class FakeWebSocket {
      static OPEN = 1
      readyState = 1
      onopen: (() => void) | null = null
      onclose: (() => void) | null = null
      onerror: (() => void) | null = null
      onmessage: ((event: MessageEvent) => void) | null = null
      constructor() { queueMicrotask(() => this.onopen?.()) }
      send() {}
      close() { this.onclose?.() }
    }
    vi.stubGlobal('WebSocket', FakeWebSocket)
    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input)
      if (path === '/api/v1/auth/me') return new Response(JSON.stringify({ user: { id: 'user-1', username: 'alice', created_at: '2026-11-15T00:00:00Z' } }))
      if (path === '/api/v1/auth/profile') return new Response(JSON.stringify({ profile: { id: 'user-1', username: 'alice', display_name: 'Alice', bio: '', updated_at: '2026-11-15T00:00:00Z' } }))
      if (path.includes('/conversations') || path === '/api/v1/communities') return new Response(JSON.stringify({ conversations: [], communities: [] }))
      return new Response(JSON.stringify({}))
    }))

    render(<App />)
    expect(await screen.findByRole('heading', { name: 'Messages' })).toBeInTheDocument()

    const results = await axe.run(document.body, {
      runOnly: { type: 'tag', values: ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa'] },
    })
    expect(results.violations.map(({ id, help, nodes }) => ({
      id,
      help,
      elements: nodes.map(({ target }) => target),
    }))).toEqual([])

    fireEvent.keyDown(window, { key: 'k', ctrlKey: true })
    expect(await screen.findByRole('dialog')).toBeInTheDocument()
    const dialogResults = await axe.run(document.body, {
      runOnly: { type: 'tag', values: ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa'] },
    })
    expect(dialogResults.violations.map(({ id, help, nodes }) => ({
      id,
      help,
      elements: nodes.map(({ target }) => target),
    }))).toEqual([])
  })
})
