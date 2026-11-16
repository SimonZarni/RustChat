import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { MfaSection } from './MfaSection'

afterEach(() => { cleanup(); vi.unstubAllGlobals() })

const json = (body: unknown, status = 200) => new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } })

function open() {
  const details = screen.getByText('Two-step verification').closest('details')!
  details.open = true
  fireEvent(details, new Event('toggle'))
}

describe('MfaSection', () => {
  it('explains when the server cannot enrol authenticators', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => json({ mfa: { available: false, enabled: false, recovery_codes_remaining: 0 } })))
    render(<MfaSection onError={() => {}} />)
    open()
    expect(await screen.findByText(/has not been set up for authenticator apps/)).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Set up' })).not.toBeInTheDocument()
  })

  it('walks through setup, confirms with a code and shows the recovery codes once', async () => {
    let enabled = false
    const calls: { url: string; body: unknown }[] = []
    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input)
      calls.push({ url, body: init?.body ? JSON.parse(String(init.body)) : undefined })
      if (url === '/api/v1/auth/mfa') return json({ mfa: { available: true, enabled, recovery_codes_remaining: enabled ? 10 : 0 } })
      if (url === '/api/v1/auth/mfa/totp') return json({ secret: 'JBSWY3DPEHPK3PXP', otpauth_uri: 'otpauth://totp/RustChat:alice?secret=JBSWY3DPEHPK3PXP' })
      if (url === '/api/v1/auth/mfa/totp/confirm') { enabled = true; return json({ recovery_codes: ['aaaa-bbbb-cccc-dddd', 'eeee-ffff-gggg-hhhh'] }) }
      return json({})
    }))
    render(<MfaSection onError={() => {}} />)
    open()

    fireEvent.change(await screen.findByLabelText('Current password'), { target: { value: 'correct horse battery' } })
    fireEvent.click(screen.getByRole('button', { name: 'Set up' }))
    expect(await screen.findByText('JBSWY3DPEHPK3PXP')).toBeInTheDocument()

    fireEvent.change(screen.getByLabelText('Code from your app'), { target: { value: '123456' } })
    fireEvent.click(screen.getByRole('button', { name: 'Turn on' }))
    expect(await screen.findByText('aaaa-bbbb-cccc-dddd')).toBeInTheDocument()
    expect(screen.getByText(/will not be shown again/)).toBeInTheDocument()
    expect(await screen.findByText(/10 recovery codes left/)).toBeInTheDocument()
    expect(calls.find((call) => call.url.endsWith('/totp/confirm'))?.body).toEqual({ code: '123456' })

    fireEvent.click(screen.getByRole('button', { name: 'I have saved them' }))
    expect(screen.queryByText('aaaa-bbbb-cccc-dddd')).not.toBeInTheDocument()
  })

  it('turns two-step verification off with the password and a code, and reports a bad code', async () => {
    let enabled = true
    const onError = vi.fn()
    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input)
      if (url === '/api/v1/auth/mfa') return json({ mfa: { available: true, enabled, recovery_codes_remaining: 4 } })
      if (url === '/api/v1/auth/mfa/disable') {
        const body = JSON.parse(String(init?.body)) as { code: string }
        if (body.code !== '654321') return json({ error: { code: 'INVALID_MFA_CODE', message: 'The verification code is invalid or has expired.' } }, 401)
        enabled = false
        return new Response(null, { status: 204 })
      }
      return json({})
    }))
    render(<MfaSection onError={onError} />)
    open()

    expect(await screen.findByText(/4 recovery codes left/)).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Turn off' })).toBeDisabled()
    fireEvent.change(screen.getByLabelText('Current password'), { target: { value: 'correct horse battery' } })
    fireEvent.change(screen.getByLabelText('Code or recovery code'), { target: { value: '000000' } })
    fireEvent.click(screen.getByRole('button', { name: 'Turn off' }))
    await waitFor(() => expect(onError).toHaveBeenCalledWith('The verification code is invalid or has expired.'))

    fireEvent.change(screen.getByLabelText('Code or recovery code'), { target: { value: '654321' } })
    fireEvent.click(screen.getByRole('button', { name: 'Turn off' }))
    expect(await screen.findByText('Two-step verification is off.')).toBeInTheDocument()
    expect(await screen.findByRole('button', { name: 'Set up' })).toBeInTheDocument()
  })
})
