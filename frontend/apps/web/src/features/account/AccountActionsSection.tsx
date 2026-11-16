import { useState } from 'react'
import { api, errorMessage } from '../../lib/api'

type Props = {
  onError: (message: string) => void
  /** Called after the account was deactivated or deleted and the session ended. */
  onAccountClosed: (notice: string) => void
}

export function AccountActionsSection({ onError, onAccountClosed }: Props) {
  const [password, setPassword] = useState('')
  const [confirmation, setConfirmation] = useState('')
  const [busy, setBusy] = useState(false)

  async function manageAccount(action: 'deactivate' | 'delete') {
    if (!password || (action === 'delete' && confirmation !== 'DELETE')) return
    setBusy(true)
    onError('')
    try {
      await api(`/api/v1/auth/${action}`, {
        method: 'POST',
        body: JSON.stringify({ current_password: password, ...(action === 'delete' ? { confirm: true } : {}) }),
      })
      onAccountClosed(action === 'deactivate'
        ? 'Account deactivated. Sign in to reactivate it.'
        : 'Account deleted. Its messages remain in conversation history.')
    } catch (cause) {
      onError(errorMessage(cause, `Could not ${action} account`))
      setBusy(false)
    }
  }

  return (
    <details className="security-settings">
      <summary>Account actions</summary>
      <section>
        <p className="muted">Deactivation signs out all sessions. You can reactivate by signing in again. Transfer community ownership before either action.</p>
        <label>Current password<input type="password" autoComplete="current-password" maxLength={128} value={password} onChange={(event) => setPassword(event.target.value)} /></label>
        <button type="button" className="outline-button" disabled={!password || busy} onClick={() => void manageAccount('deactivate')}>{busy ? 'Working…' : 'Deactivate account'}</button>
        <label>Type DELETE to permanently delete your account<input autoComplete="off" value={confirmation} onChange={(event) => setConfirmation(event.target.value)} /></label>
        <button type="button" className="account-delete-button" disabled={!password || confirmation !== 'DELETE' || busy} onClick={() => void manageAccount('delete')}>Delete account and scrub profile</button>
      </section>
    </details>
  )
}
