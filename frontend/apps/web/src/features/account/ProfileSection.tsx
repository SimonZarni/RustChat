import { useState, type Dispatch, type FormEvent, type SetStateAction } from 'react'
import { api, errorMessage } from '../../lib/api'
import type { User, UserProfile } from '../../types'

type Props = {
  user: User
  profile: UserProfile
  onProfileChange: Dispatch<SetStateAction<UserProfile | null>>
  onUserChange: (user: User) => void
  onError: (message: string) => void
}

export function ProfileSection({ user, profile, onProfileChange, onUserChange, onError }: Props) {
  const [busy, setBusy] = useState(false)

  async function saveProfile(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    setBusy(true)
    try {
      const result = await api<{ profile: UserProfile }>('/api/v1/auth/profile', {
        method: 'PATCH',
        body: JSON.stringify({ display_name: profile.display_name, bio: profile.bio }),
      })
      onProfileChange(result.profile)
    } catch (cause) {
      onError(errorMessage(cause, 'Could not save profile'))
    } finally {
      setBusy(false)
    }
  }

  async function saveUsername() {
    setBusy(true)
    onError('')
    try {
      const result = await api<{ user: User }>('/api/v1/auth/username', {
        method: 'PATCH',
        body: JSON.stringify({ username: profile.username }),
      })
      onUserChange(result.user)
      onProfileChange({ ...profile, username: result.user.username })
    } catch (cause) {
      onError(errorMessage(cause, 'Could not update username'))
    } finally {
      setBusy(false)
    }
  }

  return (
    <details className="profile-settings">
      <summary aria-label="Edit profile" title="Edit profile">Edit</summary>
      <form className="profile-form" onSubmit={saveProfile}>
        <label>Username<input minLength={3} maxLength={32} pattern="[A-Za-z0-9_]+" required value={profile.username} onChange={(event) => onProfileChange({ ...profile, username: event.target.value })} /></label>
        <button type="button" className="outline-button" disabled={busy || profile.username === user.username} onClick={() => void saveUsername()}>Update username</button>
        <label>Display name<input maxLength={64} value={profile.display_name} onChange={(event) => onProfileChange({ ...profile, display_name: event.target.value })} /></label>
        <label>About you<textarea maxLength={280} rows={3} value={profile.bio} onChange={(event) => onProfileChange({ ...profile, bio: event.target.value })} /></label>
        <button className="primary-button" disabled={busy}>{busy ? 'Saving…' : 'Save profile'}</button>
      </form>
    </details>
  )
}
