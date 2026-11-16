import { useEffect, useState } from 'react'
import { api, errorMessage } from '../../lib/api'
import { initial } from '../../lib/ids'
import type { Theme } from '../../lib/useTheme'
import type { User, UserProfile } from '../../types'
import { AccountActionsSection } from './AccountActionsSection'
import { EmailSection } from './EmailSection'
import { MfaSection } from './MfaSection'
import { InstanceAdminSection } from './InstanceAdminSection'
import { PrivacySection } from './PrivacySection'
import { ProfileSection } from './ProfileSection'
import { SecuritySection } from './SecuritySection'

type Props = {
  user: User
  theme: Theme
  onThemeChange: (theme: Theme) => void
  onUserChange: (user: User) => void
  onSignOut: () => void
  onAccountClosed: (notice: string) => void
  onError: (message: string) => void
}

/** Sidebar footer: profile, appearance, security, privacy, account and instance administration. */
export function AccountFooter({ user, theme, onThemeChange, onUserChange, onSignOut, onAccountClosed, onError }: Props) {
  const [profile, setProfile] = useState<UserProfile | null>(null)

  useEffect(() => {
    let alive = true
    api<{ profile: UserProfile }>('/api/v1/auth/profile')
      .then(({ profile: current }) => { if (alive) setProfile(current) })
      .catch((cause: unknown) => { if (alive) onError(errorMessage(cause, 'Could not load profile')) })
    return () => { alive = false }
  }, [user.id, onError])

  return (
    <footer className="sidebar-profile">
      <div className="profile-avatar">{initial(user.username)}</div>
      <div className="profile-copy"><strong>{profile?.display_name || user.username}</strong><span>@{user.username}</span></div>
      {profile && <ProfileSection user={user} profile={profile} onProfileChange={setProfile} onUserChange={onUserChange} onError={onError} />}
      <details className="security-settings">
        <summary>Appearance</summary>
        <section>
          <label>Theme
            <select aria-label="Theme" value={theme} onChange={(event) => onThemeChange(event.target.value as Theme)}>
              <option value="light">Light</option>
              <option value="dark">Dark</option>
            </select>
          </label>
        </section>
      </details>
      <SecuritySection onError={onError} />
      <EmailSection onError={onError} />
      <MfaSection onError={onError} />
      <AccountActionsSection onError={onError} onAccountClosed={onAccountClosed} />
      <PrivacySection onError={onError} />
      <InstanceAdminSection onError={onError} />
      <button className="signout-button" onClick={onSignOut}>Sign out</button>
    </footer>
  )
}
