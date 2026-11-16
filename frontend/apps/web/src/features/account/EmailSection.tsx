import { useState, type FormEvent } from 'react'
import { api, errorMessage } from '../../lib/api'
import type { EmailStatus } from '../../types'

export function EmailSection({ onError }: { onError: (message: string) => void }) {
  const [status, setStatus] = useState<EmailStatus | null>(null)
  const [email, setEmail] = useState('')
  const [password, setPassword] = useState('')
  const [busy, setBusy] = useState(false)
  const [message, setMessage] = useState('')

  async function load() {
    try {
      const result = await api<{ email: EmailStatus }>('/api/v1/auth/email')
      setStatus(result.email)
    } catch (cause) {
      onError(errorMessage(cause, 'Could not load your email address'))
    }
  }

  async function save(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    setBusy(true)
    setMessage('')
    try {
      const result = await api<{ email: EmailStatus }>('/api/v1/auth/email', {
        method: 'PUT',
        body: JSON.stringify({ email: email.trim() || null, current_password: password }),
      })
      setStatus(result.email)
      setEmail('')
      setPassword('')
      setMessage(result.email.email ? 'Address saved. Check your inbox for a confirmation link.' : 'Address removed.')
    } catch (cause) {
      onError(errorMessage(cause, 'Could not update your email address'))
    } finally {
      setBusy(false)
    }
  }

  async function resend() {
    setBusy(true)
    setMessage('')
    try {
      await api('/api/v1/auth/email/resend', { method: 'POST' })
      setMessage('Confirmation email sent.')
    } catch (cause) {
      onError(errorMessage(cause, 'Could not send the confirmation email'))
    } finally {
      setBusy(false)
    }
  }

  return (
    <details className="security-settings" onToggle={(event) => { if (event.currentTarget.open) void load() }}>
      <summary>Email</summary>
      <section>
        {status === null ? <p className="muted">Loading email settings...</p> : (
          <>
            <p className="muted">
              {status.email
                ? <>Recovery address: <strong>{status.email}</strong> · {status.verified ? 'Confirmed' : 'Not confirmed'}</>
                : 'No recovery email is set.'}
            </p>
            {status.email && !status.verified && (
              <>
                <p className="muted">Password recovery is only sent to a confirmed address.</p>
                <button type="button" className="outline-button" disabled={busy} onClick={() => void resend()}>Send confirmation email</button>
              </>
            )}
            <form className="security-form" onSubmit={save}>
              <label>New email address<input type="email" autoComplete="email" maxLength={254} value={email} onChange={(event) => setEmail(event.target.value)} /></label>
              <label>Current password<input type="password" autoComplete="current-password" maxLength={128} required value={password} onChange={(event) => setPassword(event.target.value)} /></label>
              <button className="primary-button" disabled={busy}>{email.trim() ? 'Change address' : 'Remove address'}</button>
            </form>
          </>
        )}
        {message && <p className="notice success" role="status">{message}</p>}
      </section>
    </details>
  )
}
