import { useState, type Dispatch, type FormEvent, type SetStateAction } from 'react'
import { useDialogs } from '../../components/dialogs-context'
import { api, errorMessage } from '../../lib/api'
import { ROLE_PERMISSIONS } from '../../lib/permissions'
import type { Channel, CommunityRole, User } from '../../types'

type Props = {
  communityId: string
  currentUserId: string
  roles: CommunityRole[]
  setRoles: Dispatch<SetStateAction<CommunityRole[]>>
  members: User[]
  channels: Channel[]
  selectedRoleId: string
  setSelectedRoleId: (id: string) => void
  selectedMemberId: string
  setSelectedMemberId: (id: string) => void
  overrideChannelId: string
  setOverrideChannelId: (id: string) => void
  onError: (message: string) => void
}

function PermissionGrid({ value, onToggle }: { value: number; onToggle: (bit: number, checked: boolean) => void }) {
  return (
    <div className="permission-grid">
      {ROLE_PERMISSIONS.map(([label, bit]) => (
        <label key={bit}><input type="checkbox" checked={(value & bit) !== 0} onChange={(event) => onToggle(bit, event.target.checked)} />{label}</label>
      ))}
    </div>
  )
}

export function RolesSection({
  communityId, currentUserId, roles, setRoles, members, channels,
  selectedRoleId, setSelectedRoleId, selectedMemberId, setSelectedMemberId,
  overrideChannelId, setOverrideChannelId, onError,
}: Props) {
  const dialogs = useDialogs()
  const [newName, setNewName] = useState('')
  const [newPermissions, setNewPermissions] = useState(193)
  const [overrideAllow, setOverrideAllow] = useState(0)
  const [overrideDeny, setOverrideDeny] = useState(0)

  const editRole = (id: string, change: (role: CommunityRole) => CommunityRole) =>
    setRoles((current) => current.map((item) => item.id === id ? change(item) : item))

  async function create(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    try {
      const result = await api<{ role: CommunityRole }>(`/api/v1/communities/${communityId}/roles`, { method: 'POST', body: JSON.stringify({ name: newName, permissions: newPermissions }) })
      setRoles((current) => [...current, result.role].sort((a, b) => a.position - b.position))
      setSelectedRoleId(result.role.id)
      setNewName('')
    } catch (cause) { onError(errorMessage(cause, 'Could not create role')) }
  }

  async function save(role: CommunityRole) {
    if (role.is_default) return
    try {
      const result = await api<{ role: CommunityRole }>(`/api/v1/communities/${communityId}/roles/${role.id}`, { method: 'PATCH', body: JSON.stringify({ name: role.name, permissions: role.permissions, position: role.position }) })
      setRoles((current) => current.map((item) => item.id === role.id ? result.role : item))
    } catch (cause) { onError(errorMessage(cause, 'Could not update role')) }
  }

  async function remove(role: CommunityRole) {
    if (role.is_default || !await dialogs.confirm({ title: `Delete the ${role.name} role?`, detail: 'Members lose the permissions it granted.', confirmLabel: 'Delete role', danger: true })) return
    try {
      await api(`/api/v1/communities/${communityId}/roles/${role.id}`, { method: 'DELETE' })
      setRoles((current) => current.filter((item) => item.id !== role.id))
      if (selectedRoleId === role.id) setSelectedRoleId('')
    } catch (cause) { onError(errorMessage(cause, 'Could not delete role')) }
  }

  async function assign(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!selectedRoleId || !selectedMemberId) return
    try {
      await api(`/api/v1/communities/${communityId}/members/${selectedMemberId}/roles/${selectedRoleId}`, { method: 'POST' })
    } catch (cause) { onError(errorMessage(cause, 'Could not assign role')) }
  }

  async function unassign() {
    if (!selectedMemberId || !selectedRoleId) return
    try {
      await api(`/api/v1/communities/${communityId}/members/${selectedMemberId}/roles/${selectedRoleId}`, { method: 'DELETE' })
    } catch (cause) { onError(errorMessage(cause, 'Could not remove role')) }
  }

  async function saveOverride(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!selectedRoleId || !overrideChannelId) return
    try {
      await api(`/api/v1/channels/${overrideChannelId}/permissions/${selectedRoleId}`, { method: 'PUT', body: JSON.stringify({ allow: overrideAllow, deny: overrideDeny }) })
    } catch (cause) { onError(errorMessage(cause, 'Could not update channel permissions')) }
  }

  const roleOptions = roles.map((role) => <option key={role.id} value={role.id}>{role.name}</option>)

  return (
    <section className="manager-section">
      <h3>Roles and permissions</h3>
      <form className="role-create-form" onSubmit={create}>
        <label>New role<input required minLength={2} maxLength={32} value={newName} onChange={(event) => setNewName(event.target.value)} /></label>
        <PermissionGrid value={newPermissions} onToggle={(bit, checked) => setNewPermissions((current) => checked ? current | bit : current & ~bit)} />
        <button className="outline-button">Create role</button>
      </form>
      {roles.map((role) => (
        <article className="role-editor" key={role.id}>
          <div className="role-editor-heading">
            <strong>{role.name}{role.is_default ? ' · default' : ''}</strong>
            {!role.is_default && <>
              <button type="button" onClick={() => void save(role)}>Save role</button>
              <button type="button" onClick={() => void remove(role)}>Delete role</button>
            </>}
          </div>
          {!role.is_default && <>
            <label>Role name<input value={role.name} maxLength={32} onChange={(event) => editRole(role.id, (item) => ({ ...item, name: event.target.value }))} /></label>
            <label>Position<input type="number" min={1} value={role.position} onChange={(event) => editRole(role.id, (item) => ({ ...item, position: Number(event.target.value) }))} /></label>
            <PermissionGrid value={role.permissions} onToggle={(bit, checked) => editRole(role.id, (item) => ({ ...item, permissions: checked ? item.permissions | bit : item.permissions & ~bit }))} />
          </>}
        </article>
      ))}
      <form className="role-assign-form" onSubmit={assign}>
        <h4>Assign a role</h4>
        <label>Member
          <select required value={selectedMemberId} onChange={(event) => setSelectedMemberId(event.target.value)}>
            <option value="">Choose member</option>
            {members.filter((member) => member.id !== currentUserId).map((member) => <option key={member.id} value={member.id}>{member.username}</option>)}
          </select>
        </label>
        <label>Role<select required value={selectedRoleId} onChange={(event) => setSelectedRoleId(event.target.value)}>{roleOptions}</select></label>
        <button className="outline-button">Assign role</button>
        <button type="button" className="outline-button" onClick={() => void unassign()}>Remove role</button>
      </form>
      <form className="role-assign-form" onSubmit={saveOverride}>
        <h4>Channel permission override</h4>
        <label>Channel
          <select required value={overrideChannelId} onChange={(event) => setOverrideChannelId(event.target.value)}>
            {channels.map((channel) => <option key={channel.id} value={channel.id}>{channel.name}</option>)}
          </select>
        </label>
        <label>Role<select required value={selectedRoleId} onChange={(event) => setSelectedRoleId(event.target.value)}>{roleOptions}</select></label>
        <label>Allow bits<input type="number" min={0} max={511} value={overrideAllow} onChange={(event) => setOverrideAllow(Number(event.target.value))} /></label>
        <label>Deny bits<input type="number" min={0} max={511} value={overrideDeny} onChange={(event) => setOverrideDeny(Number(event.target.value))} /></label>
        <button className="outline-button">Save override</button>
      </form>
    </section>
  )
}
