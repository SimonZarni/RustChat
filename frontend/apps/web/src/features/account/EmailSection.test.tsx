import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { EmailSection } from './EmailSection'

afterEach(() => { cleanup(); vi.unstubAllGlobals() })

const json = (body: unknown, status = 200) => new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } })

function open() {
  fireEvent.click(screen.getByText('Email'))
  const details = screen.getByText('Email').closest('details')!
  details.open = true
  fireEvent(details, new Event('toggle'))
}

describe('EmailSection', () => {
  it('shows an unconfirmed address, can resend the link and can change the address with the password', async () => {
    const calls: { url: string; method: string; body: unknown }[] = []
    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input)
      calls.push({ url, method: init?.method ?? 'GET', body: init?.body ? JSON.parse(String(init.body)) : undefined })
      if (url === '/api/v1/auth/email' && (init?.method ?? 'GET') === 'GET') return json({ email: { email: 'alice@example.test', verified: false } })
      if (url === '/api/v1/auth/email/resend') return new Response(null, { status: 202 })
      if (url === '/api/v1/auth/email') return json({ email: { email: 'new@example.test', verified: false } })
      return json({})
    }))
    const onError = vi.fn()
    render(<EmailSection onError={onError} />)
    open()

    expect(await screen.findByText('alice@example.test')).toBeInTheDocument()
    expect(screen.getByText(/Not confirmed/)).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'Send confirmation email' }))
    expect(await screen.findByText('Confirmation email sent.')).toBeInTheDocument()

    fireEvent.change(screen.getByLabelText('New email address'), { target: { value: 'new@example.test' } })
    fireEvent.change(screen.getByLabelText('Current password'), { target: { value: 'correct horse battery' } })
    fireEvent.click(screen.getByRole('button', { name: 'Change address' }))
    expect(await screen.findByText(/Address saved/)).toBeInTheDocument()
    expect(calls.find((call) => call.method === 'PUT')?.body).toEqual({ email: 'new@example.test', current_password: 'correct horse battery' })
    expect(screen.getByLabelText('Current password')).toHaveValue('')
    expect(onError).not.toHaveBeenCalled()
  })

  it('hides the resend button once the address is confirmed and reports server errors', async () => {
    vi.stubGlobal('fetch', vi.fn(async (_input: RequestInfo | URL, init?: RequestInit) => {
      if ((init?.method ?? 'GET') === 'PUT') return json({ error: { code: 'INVALID_CREDENTIALS', message: 'Invalid credentials.' } }, 401)
      return json({ email: { email: 'alice@example.test', verified: true } })
    }))
    const onError = vi.fn()
    render(<EmailSection onError={onError} />)
    open()

    expect(await screen.findByText(/Confirmed/)).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Send confirmation email' })).not.toBeInTheDocument()

    fireEvent.change(screen.getByLabelText('Current password'), { target: { value: 'wrong password!!' } })
    fireEvent.click(screen.getByRole('button', { name: 'Remove address' }))
    await waitFor(() => expect(onError).toHaveBeenCalledWith('Invalid credentials.'))
  })
})
