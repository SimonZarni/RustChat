import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { AuditEvent } from '../../types'
import { describeAuditEvent } from './describe'
import { AuditLogPanel } from './AuditLogPanel'

afterEach(() => { cleanup(); vi.unstubAllGlobals() })

const event = (id: string, type: string, extra: Partial<AuditEvent> = {}): AuditEvent => ({
  id, event_type: type, actor: { id: 'u1', username: 'alice' }, target: { id: 'u2', username: 'bobby' }, details: {}, created_at: '2026-11-16T10:00:00Z', ...extra,
})
const json = (body: unknown, status = 200) => new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } })

describe('describeAuditEvent', () => {
  it('names the actor and target, and falls back to the event type', () => {
    expect(describeAuditEvent(event('1', 'user.suspended'))).toBe('alice suspended bobby')
    expect(describeAuditEvent(event('1', 'invite.created', { target: null }))).toBe('alice created an invitation')
    expect(describeAuditEvent(event('1', 'something.new', { actor: null }))).toBe('something.new')
  })
})

describe('AuditLogPanel', () => {
  it('loads on demand, then pages with the cursor', async () => {
    const urls: string[] = []
    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL) => {
      urls.push(String(input))
      return String(input).includes('before=c1')
        ? json({ events: [event('3', 'user.unsuspended')], next_cursor: null })
        : json({ events: [event('1', 'user.suspended'), event('2', 'user.suspended')], next_cursor: 'c1' })
    }))
    render(<AuditLogPanel path="/api/v1/admin/audit" title="Administration history" onError={() => {}} />)
    expect(urls).toHaveLength(0)

    fireEvent.click(screen.getByRole('button', { name: 'Show history' }))
    expect(await screen.findAllByText('alice suspended bobby')).toHaveLength(2)
    fireEvent.click(screen.getByRole('button', { name: 'Load more' }))
    expect(await screen.findByText('alice restored bobby')).toBeInTheDocument()
    expect(urls).toEqual(['/api/v1/admin/audit?limit=25', '/api/v1/admin/audit?before=c1&limit=25'])
    expect(screen.queryByRole('button', { name: 'Load more' })).not.toBeInTheDocument()
    expect(screen.getByRole('list', { name: 'Administration history' })).toBeInTheDocument()
  })

  it('says so when the caller may not read the history and reports other errors', async () => {
    const onError = vi.fn()
    vi.stubGlobal('fetch', vi.fn(async () => json({ error: { code: 'NOT_FOUND', message: 'Not found.' } }, 404)))
    render(<AuditLogPanel path="/x" title="Audit history" onError={onError} />)
    fireEvent.click(screen.getByRole('button', { name: 'Show history' }))
    expect(await screen.findByText('You do not have access to this history.')).toBeInTheDocument()
    expect(onError).not.toHaveBeenCalled()
  })
})
