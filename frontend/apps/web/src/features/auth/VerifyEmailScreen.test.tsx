import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { StrictMode } from 'react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { readVerifyEmailToken } from '../../lib/links'
import { VerifyEmailScreen } from './VerifyEmailScreen'

afterEach(() => { cleanup(); vi.unstubAllGlobals(); window.history.replaceState({}, '', '/') })

describe('readVerifyEmailToken', () => {
  it('reads the token only on the verification page', () => {
    window.history.replaceState({}, '', '/verify-email?token=abc123')
    expect(readVerifyEmailToken()).toBe('abc123')
    window.history.replaceState({}, '', '/verify-email')
    expect(readVerifyEmailToken()).toBe('')
    window.history.replaceState({}, '', '/?token=abc123')
    expect(readVerifyEmailToken()).toBeNull()
  })
})

describe('VerifyEmailScreen', () => {
  it('confirms the address once, even when effects run twice, and continues to the app', async () => {
    const fetchMock = vi.fn(async () => new Response(null, { status: 204 }))
    vi.stubGlobal('fetch', fetchMock)
    const onContinue = vi.fn()
    render(<StrictMode><VerifyEmailScreen token="tok-1" onContinue={onContinue} /></StrictMode>)

    expect(await screen.findByRole('heading', { name: 'Address confirmed' })).toBeInTheDocument()
    expect(fetchMock).toHaveBeenCalledTimes(1)
    const [url, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit]
    expect(url).toBe('/api/v1/auth/email/verify')
    expect(JSON.parse(String(init.body))).toEqual({ token: 'tok-1' })

    fireEvent.click(screen.getByRole('button', { name: 'Continue to RustChat' }))
    expect(onContinue).toHaveBeenCalled()
  })

  it('shows the server message when the link is invalid or expired', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response(
      JSON.stringify({ error: { code: 'INVALID_ACCOUNT_TOKEN', message: 'The account link is invalid or expired. Request a new one.' } }),
      { status: 400, headers: { 'Content-Type': 'application/json' } },
    )))
    render(<VerifyEmailScreen token="old" onContinue={() => {}} />)
    expect(await screen.findByRole('alert')).toHaveTextContent('invalid or expired')
    expect(screen.getByRole('heading', { name: 'Could not confirm' })).toBeInTheDocument()
  })

  it('does not call the server for a link without a token', async () => {
    const fetchMock = vi.fn()
    vi.stubGlobal('fetch', fetchMock)
    render(<VerifyEmailScreen token="" onContinue={() => {}} />)
    expect(await screen.findByRole('alert')).toHaveTextContent('incomplete')
    expect(fetchMock).not.toHaveBeenCalled()
  })
})
