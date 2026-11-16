import { useEffect, useState } from 'react'
import { api, errorMessage } from '../../lib/api'
import type { InstanceUser } from '../../types'

function accountState(item: InstanceUser): string {
  return item.deleted_at ? 'Deleted' : item.suspended_at ? 'Suspended' : item.deactivated_at ? 'Deactivated' : 'Active'
}

/** Renders only for configured instance administrators (the admin API answers 403 to everyone else). */
export function InstanceAdminSection({ onError }: { onError: (message: string) => void }) {
  const [users, setUsers] = useState<InstanceUser[] | null>(null)
  const [message, setMessage] = useState('')

  useEffect(() => {
    let alive = true
    api<{ users: InstanceUser[] }>('/api/v1/admin/users')
      .then(({ users: items }) => { if (alive) setUsers(items) })
      .catch(() => { if (alive) setUsers(null) })
    return () => { alive = false }
  }, [])

  async function setSuspended(target: InstanceUser) {
    try {
      const { user: updated } = await api<{ user: InstanceUser }>(`/api/v1/admin/users/${target.id}/suspension`, {
        method: 'PUT',
        body: JSON.stringify({ suspended: !target.suspended_at }),
      })
      setUsers((current) => current?.map((item) => item.id === updated.id ? updated : item) ?? null)
      setMessage(updated.suspended_at ? `${updated.username} suspended.` : `${updated.username} restored.`)
    } catch (cause) {
      onError(errorMessage(cause, 'Could not update this account'))
    }
  }

  if (!users) return null
  return (
    <details className="security-settings">
      <summary>Instance administration</summary>
      <section>
        <p className="muted">The newest 100 accounts are shown. Suspension revokes sessions and prevents sign-in.</p>
        {message && <p className="notice success" role="status">{message}</p>}
        <div className="instance-user-list">
          {users.map((item) => (
            <div className="manager-row" key={item.id}>
              <span>{item.username} · {accountState(item)}</span>
              {item.is_instance_admin
                ? <small>Configured administrator</small>
                : !item.deleted_at && <button type="button" aria-label={`${item.suspended_at ? 'Restore' : 'Suspend'} ${item.username}`} onClick={() => void setSuspended(item)}>{item.suspended_at ? 'Restore' : 'Suspend'}</button>}
            </div>
          ))}
        </div>
      </section>
    </details>
  )
}
