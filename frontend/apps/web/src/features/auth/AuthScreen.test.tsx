import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { AuthScreen } from './AuthScreen'

afterEach(() => { cleanup(); vi.unstubAllGlobals() })

const json = (body: unknown, status = 200) => new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } })
const user = { id: 'u1', username: 'alice', created_at: '2026-11-16T00:00:00Z' }

function signInWithPassword() {
  fireEvent.change(screen.getByLabelText('Username'), { target: { value: 'alice' } })
  fireEvent.change(screen.getByLabelText('Password'), { target: { value: 'correct horse battery' } })
  fireEvent.click(screen.getByRole('button', { name: 'Sign in' }))
}

describe('AuthScreen two-step sign-in', () => {
  it('asks for a code after the password and finishes the sign-in with it', async () => {
    const calls: { url: string; body: unknown }[] = []
    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input)
      calls.push({ url, body: init?.body ? JSON.parse(String(init.body)) : undefined })
      if (url === '/api/v1/auth/login') return json({ mfa_required: true, mfa_token: 'challenge-1' }, 202)
      if (url === '/api/v1/auth/login/mfa') return json({ user })
      return json({})
    }))
    const onAuthenticated = vi.fn()
    render(<AuthScreen onAuthenticated={onAuthenticated} />)

    signInWithPassword()
    expect(await screen.findByRole('heading', { name: 'Verification code' })).toBeInTheDocument()
    expect(onAuthenticated).not.toHaveBeenCalled()
    // The password is not kept around for the second step.
    expect(screen.queryByLabelText('Password')).not.toBeInTheDocument()

    fireEvent.change(screen.getByLabelText('Code'), { target: { value: '123456' } })
    fireEvent.click(screen.getByRole('button', { name: 'Verify and sign in' }))
    await waitFor(() => expect(onAuthenticated).toHaveBeenCalledWith(user))
    expect(calls.at(-1)).toEqual({ url: '/api/v1/auth/login/mfa', body: { mfa_token: 'challenge-1', code: '123456' } })
  })

  it('shows a rejected code, clears the field and lets the user go back', async () => {
    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL) => String(input) === '/api/v1/auth/login'
      ? json({ mfa_required: true, mfa_token: 'challenge-1' }, 202)
      : json({ error: { code: 'INVALID_MFA_CODE', message: 'The verification code is invalid or has expired.' } }, 401)))
    const onAuthenticated = vi.fn()
    render(<AuthScreen onAuthenticated={onAuthenticated} />)

    signInWithPassword()
    fireEvent.change(await screen.findByLabelText('Code'), { target: { value: '000000' } })
    fireEvent.click(screen.getByRole('button', { name: 'Verify and sign in' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('invalid or has expired')
    expect(screen.getByLabelText('Code')).toHaveValue('')
    expect(onAuthenticated).not.toHaveBeenCalled()

    fireEvent.click(screen.getByRole('button', { name: 'Back to sign in' }))
    expect(await screen.findByRole('heading', { name: 'Welcome back' })).toBeInTheDocument()
  })

  it('signs in directly when the account has no second factor', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => json({ user })))
    const onAuthenticated = vi.fn()
    render(<AuthScreen onAuthenticated={onAuthenticated} />)
    signInWithPassword()
    await waitFor(() => expect(onAuthenticated).toHaveBeenCalledWith(user))
  })
})
