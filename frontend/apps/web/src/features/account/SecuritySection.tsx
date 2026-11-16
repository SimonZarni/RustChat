import { useState, type FormEvent } from 'react'
import { api, errorMessage } from '../../lib/api'
import type { Session } from '../../types'

export function SecuritySection({ onError }: { onError: (message: string) => void }) {
  const [sessions, setSessions] = useState<Session[]>([])
  const [currentPassword, setCurrentPassword] = useState('')
  const [newPassword, setNewPassword] = useState('')
  const [busy, setBusy] = useState(false)
  const [message, setMessage] = useState('')

  async function loadSessions() {
    try {
      const result = await api<{ sessions: Session[] }>('/api/v1/auth/sessions')
      setSessions(result.sessions)
    } catch (cause) {
      onError(errorMessage(cause, 'Could not load active sessions'))
    }
  }

  async function changePassword(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    setBusy(true)
    setMessage('')
    try {
      await api('/api/v1/auth/password', {
        method: 'POST',
        body: JSON.stringify({ current_password: currentPassword, new_password: newPassword }),
      })
      setCurrentPassword('')
      setNewPassword('')
      setMessage('Password updated. Other sessions were signed out.')
      await loadSessions()
    } catch (cause) {
      onError(errorMessage(cause, 'Could not change password'))
    } finally {
      setBusy(false)
    }
  }

  async function revokeSession(sessionId: string) {
    try {
      await api(`/api/v1/auth/sessions/${sessionId}`, { method: 'DELETE' })
      setSessions((current) => current.filter((session) => session.id !== sessionId))
    } catch (cause) {
      onError(errorMessage(cause, 'Could not revoke session'))
    }
  }

  return (
    <details className="security-settings" onToggle={(event) => { if (event.currentTarget.open) void loadSessions() }}>
      <summary>Security</summary>
      <section>
        <form className="security-form" onSubmit={changePassword}>
          <label>Current password<input type="password" autoComplete="current-password" maxLength={128} required value={currentPassword} onChange={(event) => setCurrentPassword(event.target.value)} /></label>
          <label>New password<input type="password" autoComplete="new-password" minLength={12} maxLength={128} required value={newPassword} onChange={(event) => setNewPassword(event.target.value)} /></label>
          <button className="primary-button" disabled={busy}>{busy ? 'Updating...' : 'Change password'}</button>
        </form>
        {message && <p className="notice success" role="status">{message}</p>}
        <h4>Active sessions</h4>
        {sessions.length === 0
          ? <p className="muted">No active sessions found.</p>
          : sessions.map((session) => (
            <div className="session-row" key={session.id}>
              <span><strong>{session.current ? 'This session' : 'Signed in'}</strong><small>Started {new Date(session.created_at).toLocaleString()}</small></span>
              {session.current ? <span className="current-session">Current</span> : <button type="button" onClick={() => void revokeSession(session.id)}>Revoke</button>}
            </div>
          ))}
      </section>
    </details>
  )
}
