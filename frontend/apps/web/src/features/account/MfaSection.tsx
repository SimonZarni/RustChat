import { useState, type FormEvent } from 'react'
import { api, errorMessage } from '../../lib/api'
import type { MfaStatus } from '../../types'

type Enrollment = { secret: string; otpauth_uri: string }

/** Two-step sign-in with an authenticator app (TOTP) and one-time recovery codes. */
export function MfaSection({ onError }: { onError: (message: string) => void }) {
  const [status, setStatus] = useState<MfaStatus | null>(null)
  const [password, setPassword] = useState('')
  const [code, setCode] = useState('')
  const [enrollment, setEnrollment] = useState<Enrollment | null>(null)
  /** Recovery codes are only ever shown right after they are created. */
  const [recoveryCodes, setRecoveryCodes] = useState<string[] | null>(null)
  const [busy, setBusy] = useState(false)
  const [message, setMessage] = useState('')

  async function load() {
    try {
      const result = await api<{ mfa: MfaStatus }>('/api/v1/auth/mfa')
      setStatus(result.mfa)
    } catch (cause) {
      onError(errorMessage(cause, 'Could not load two-step verification'))
    }
  }

  async function run(action: () => Promise<void>, failure: string) {
    setBusy(true)
    setMessage('')
    try {
      await action()
    } catch (cause) {
      onError(errorMessage(cause, failure))
    } finally {
      setBusy(false)
    }
  }

  const begin = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    void run(async () => {
      setEnrollment(await api<Enrollment>('/api/v1/auth/mfa/totp', { method: 'POST', body: JSON.stringify({ current_password: password }) }))
      setPassword('')
    }, 'Could not start setup')
  }

  const confirm = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    void run(async () => {
      const result = await api<{ recovery_codes: string[] }>('/api/v1/auth/mfa/totp/confirm', { method: 'POST', body: JSON.stringify({ code }) })
      setRecoveryCodes(result.recovery_codes)
      setEnrollment(null)
      setCode('')
      await load()
    }, 'That code was not accepted')
  }

  const disable = () => void run(async () => {
    await api('/api/v1/auth/mfa/disable', { method: 'POST', body: JSON.stringify({ current_password: password, code }) })
    setPassword(''); setCode(''); setRecoveryCodes(null)
    setMessage('Two-step verification is off.')
    await load()
  }, 'Could not turn off two-step verification')

  const regenerate = () => void run(async () => {
    const result = await api<{ recovery_codes: string[] }>('/api/v1/auth/mfa/recovery-codes', { method: 'POST', body: JSON.stringify({ current_password: password, code }) })
    setRecoveryCodes(result.recovery_codes)
    setPassword(''); setCode('')
    await load()
  }, 'Could not create new recovery codes')

  return (
    <details className="security-settings" onToggle={(event) => { if (event.currentTarget.open) void load() }}>
      <summary>Two-step verification</summary>
      <section>
        {status === null ? <p className="muted">Loading…</p>
          : !status.available ? <p className="muted">This server has not been set up for authenticator apps. An administrator can enable it by configuring an encryption key.</p>
            : (
              <>
                {recoveryCodes && (
                  <div className="notice success" role="status">
                    <p><strong>Save these recovery codes now.</strong> Each works once if you lose your authenticator. They will not be shown again.</p>
                    <ul aria-label="Recovery codes">{recoveryCodes.map((item) => <li key={item}><code>{item}</code></li>)}</ul>
                    <button type="button" className="outline-button" onClick={() => setRecoveryCodes(null)}>I have saved them</button>
                  </div>
                )}
                {!status.enabled && !enrollment && (
                  <form className="security-form" onSubmit={begin}>
                    <p className="muted">Add a second step to signing in with an authenticator app.</p>
                    <label>Current password<input type="password" autoComplete="current-password" maxLength={128} required value={password} onChange={(event) => setPassword(event.target.value)} /></label>
                    <button className="primary-button" disabled={busy}>Set up</button>
                  </form>
                )}
                {enrollment && (
                  <form className="security-form" onSubmit={confirm}>
                    <p className="muted">Add this account in your authenticator app using the setup key, then enter the six-digit code it shows.</p>
                    <p>Setup key: <code>{enrollment.secret}</code></p>
                    <p className="muted">Or open this link on your phone: <a href={enrollment.otpauth_uri}>add to authenticator</a></p>
                    <label>Code from your app<input inputMode="numeric" autoComplete="one-time-code" pattern="[0-9]{6}" maxLength={6} required value={code} onChange={(event) => setCode(event.target.value)} /></label>
                    <button className="primary-button" disabled={busy}>Turn on</button>
                  </form>
                )}
                {status.enabled && (
                  <div className="security-form">
                    <p className="muted">Two-step verification is on. {status.recovery_codes_remaining} recovery {status.recovery_codes_remaining === 1 ? 'code' : 'codes'} left.</p>
                    <label>Current password<input type="password" autoComplete="current-password" maxLength={128} value={password} onChange={(event) => setPassword(event.target.value)} /></label>
                    <label>Code or recovery code<input autoComplete="one-time-code" maxLength={32} value={code} onChange={(event) => setCode(event.target.value)} /></label>
                    <button type="button" className="outline-button" disabled={busy || !password || !code} onClick={regenerate}>Create new recovery codes</button>
                    <button type="button" className="account-delete-button" disabled={busy || !password || !code} onClick={disable}>Turn off</button>
                  </div>
                )}
              </>
            )}
        {message && <p className="notice success" role="status">{message}</p>}
      </section>
    </details>
  )
}
