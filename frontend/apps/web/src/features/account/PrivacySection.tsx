import { useState, type FormEvent } from 'react'
import { api, errorMessage } from '../../lib/api'
import type { ProfilePrivacy } from '../../types'

export function PrivacySection({ onError }: { onError: (message: string) => void }) {
  const [privacy, setPrivacy] = useState<ProfilePrivacy | null>(null)
  const [busy, setBusy] = useState(false)

  async function loadPrivacy() {
    try {
      const result = await api<{ privacy: ProfilePrivacy }>('/api/v1/auth/privacy')
      setPrivacy(result.privacy)
    } catch (cause) {
      onError(errorMessage(cause, 'Could not load privacy settings'))
    }
  }

  async function savePrivacy(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!privacy) return
    setBusy(true)
    try {
      const result = await api<{ privacy: ProfilePrivacy }>('/api/v1/auth/privacy', {
        method: 'PATCH', body: JSON.stringify(privacy),
      })
      setPrivacy(result.privacy)
    } catch (cause) {
      onError(errorMessage(cause, 'Could not update privacy settings'))
    } finally {
      setBusy(false)
    }
  }

  return (
    <details className="security-settings" onToggle={(event) => { if (event.currentTarget.open) void loadPrivacy() }}>
      <summary>Privacy</summary>
      <section>
        {privacy ? (
          <form className="security-form" onSubmit={savePrivacy}>
            <label className="privacy-toggle"><input type="checkbox" checked={privacy.discoverable} onChange={(event) => setPrivacy({ ...privacy, discoverable: event.target.checked })} />Allow people to find me by username</label>
            <label className="privacy-toggle"><input type="checkbox" checked={privacy.presence_visible} onChange={(event) => setPrivacy({ ...privacy, presence_visible: event.target.checked })} />Show my online status to conversation members</label>
            <label>Who can view my profile
              <select value={privacy.profile_visibility} onChange={(event) => setPrivacy({ ...privacy, profile_visibility: event.target.value as ProfilePrivacy['profile_visibility'] })}>
                <option value="everyone">Everyone who can find me</option>
                <option value="conversation_members">People in my conversations</option>
              </select>
            </label>
            <button className="primary-button" disabled={busy}>{busy ? 'Saving...' : 'Save privacy'}</button>
          </form>
        ) : <p className="muted">Loading privacy settings...</p>}
      </section>
    </details>
  )
}
