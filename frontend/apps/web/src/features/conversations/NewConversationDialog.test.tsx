import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { NewConversationDialog } from './NewConversationDialog'

afterEach(() => { cleanup(); vi.unstubAllGlobals() })

function setup() {
  const requests: Array<{ url: string; body: unknown }> = []
  vi.stubGlobal('fetch', vi.fn(async (url: string, init?: RequestInit) => {
    requests.push({ url, body: init?.body ? JSON.parse(String(init.body)) : undefined })
    return new Response(JSON.stringify({ conversation: { id: 'c1', kind: 'group', name: 'x', created_by: 'me', created_at: '2026-01-01T00:00:00Z', members: ['me', 'a', 'b'] } }), { status: 201 })
  }))
  const onCreated = vi.fn()
  render(<NewConversationDialog onClose={() => {}} onError={() => {}} onCreated={onCreated} />)
  fireEvent.click(screen.getByRole('button', { name: 'Group' }))
  return { requests, onCreated }
}

describe('NewConversationDialog group naming', () => {
  it('offers an optional name only for groups, focused first', () => {
    render(<NewConversationDialog onClose={() => {}} onError={() => {}} onCreated={() => {}} />)
    expect(screen.queryByLabelText(/Group name/)).not.toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Group' }))
    const name = screen.getByLabelText('Group name (optional)')
    expect(name).toHaveAttribute('maxLength', '80')
    expect(name).toHaveFocus()
  })

  it('sends the trimmed name with the members', async () => {
    const { requests, onCreated } = setup()
    fireEvent.change(screen.getByLabelText('Group name (optional)'), { target: { value: '  Weekend plans  ' } })
    fireEvent.change(screen.getByLabelText('Usernames'), { target: { value: 'bobby, carol' } })
    fireEvent.click(screen.getByRole('button', { name: 'Continue' }))
    await waitFor(() => expect(onCreated).toHaveBeenCalled())
    expect(requests[0]).toEqual({ url: '/api/v1/conversations', body: { type: 'group', usernames: ['bobby', 'carol'], name: 'Weekend plans' } })
  })

  it('leaves the name out when it is blank', async () => {
    const { requests, onCreated } = setup()
    fireEvent.change(screen.getByLabelText('Group name (optional)'), { target: { value: '   ' } })
    fireEvent.change(screen.getByLabelText('Usernames'), { target: { value: 'bobby' } })
    fireEvent.click(screen.getByRole('button', { name: 'Continue' }))
    await waitFor(() => expect(onCreated).toHaveBeenCalled())
    expect(requests[0]?.body).toEqual({ type: 'group', usernames: ['bobby'] })
  })
})
