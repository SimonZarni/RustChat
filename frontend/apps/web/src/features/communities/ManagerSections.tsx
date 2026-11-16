import { useState, type Dispatch, type FormEvent, type SetStateAction } from 'react'
import { api, errorMessage } from '../../lib/api'
import type { Category, Channel, CommunityInvite, User } from '../../types'

type ErrorProps = { onError: (message: string) => void }

export function InvitesSection({ communityId, invites, setInvites, onInviteRevoked, onError }: ErrorProps & {
  communityId: string
  invites: CommunityInvite[]
  setInvites: Dispatch<SetStateAction<CommunityInvite[]>>
  onInviteRevoked: () => void
}) {
  async function revoke(invite: CommunityInvite) {
    if (invite.revoked_at) return
    try {
      await api(`/api/v1/communities/${communityId}/invites/${invite.id}`, { method: 'DELETE' })
      setInvites((current) => current.map((item) => item.id === invite.id ? { ...item, revoked_at: new Date().toISOString() } : item))
      onInviteRevoked()
    } catch (cause) { onError(errorMessage(cause, 'Could not revoke invite')) }
  }

  const [now] = useState(() => Date.now())
  const status = (invite: CommunityInvite) => invite.revoked_at ? 'Revoked'
    : invite.expires_at && Date.parse(invite.expires_at) <= now ? 'Expired'
      : invite.max_uses !== null && invite.use_count >= invite.max_uses ? 'Used up' : 'Active'

  return (
    <section className="manager-section">
      <h3>Invitations</h3>
      {invites.length === 0 ? <p className="muted">No invitations have been created.</p> : invites.map((invite) => (
        <div className="manager-row" key={invite.id}>
          <span>{status(invite)} · {invite.use_count}{invite.max_uses === null ? '' : `/${invite.max_uses}`} uses{invite.expires_at ? ` · expires ${new Date(invite.expires_at).toLocaleDateString()}` : ''}</span>
          {!invite.revoked_at && <button type="button" onClick={() => void revoke(invite)}>Revoke</button>}
        </div>
      ))}
    </section>
  )
}

export function CategoriesSection({ communityId, categories, setCategories, setChannels, onError }: ErrorProps & {
  communityId: string
  categories: Category[]
  setCategories: Dispatch<SetStateAction<Category[]>>
  setChannels: Dispatch<SetStateAction<Channel[]>>
}) {
  const [newName, setNewName] = useState('')

  async function create(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    try {
      const result = await api<{ category: Category }>(`/api/v1/communities/${communityId}/categories`, { method: 'POST', body: JSON.stringify({ name: newName }) })
      setCategories((current) => [...current, result.category].sort((a, b) => a.position - b.position))
      setNewName('')
    } catch (cause) { onError(errorMessage(cause, 'Could not create category')) }
  }

  async function rename(category: Category) {
    const name = window.prompt('Category name', category.name)?.trim()
    if (!name) return
    try {
      const result = await api<{ category: Category }>(`/api/v1/communities/${communityId}/categories/${category.id}`, { method: 'PATCH', body: JSON.stringify({ name, position: category.position }) })
      setCategories((current) => current.map((item) => item.id === category.id ? result.category : item))
    } catch (cause) { onError(errorMessage(cause, 'Could not rename category')) }
  }

  async function remove(category: Category) {
    if (!window.confirm(`Delete category ${category.name}? Its channels will become uncategorized.`)) return
    try {
      await api(`/api/v1/communities/${communityId}/categories/${category.id}`, { method: 'DELETE' })
      setCategories((current) => current.filter((item) => item.id !== category.id))
      setChannels((current) => current.map((channel) => channel.category_id === category.id ? { ...channel, category_id: null } : channel))
    } catch (cause) { onError(errorMessage(cause, 'Could not delete category')) }
  }

  return (
    <section className="manager-section">
      <h3>Categories</h3>
      <form className="role-create-form" onSubmit={create}>
        <label>New category<input required maxLength={64} value={newName} onChange={(event) => setNewName(event.target.value)} placeholder="Projects" /></label>
        <button className="outline-button">Create category</button>
      </form>
      {categories.map((category) => (
        <div className="manager-row" key={category.id}>
          <span>{category.name}</span>
          <button type="button" onClick={() => void rename(category)}>Rename</button>
          <button type="button" onClick={() => void remove(category)}>Delete</button>
        </div>
      ))}
    </section>
  )
}

export function ChannelsSection({ communityId, channels, categories, setChannels, onChannelDeleted, onError }: ErrorProps & {
  communityId: string
  channels: Channel[]
  categories: Category[]
  setChannels: Dispatch<SetStateAction<Channel[]>>
  onChannelDeleted: (conversationId: string) => void
}) {
  async function update(channel: Channel, changes: Partial<Pick<Channel, 'name' | 'category_id'>>, failure: string) {
    try {
      const updated = await api<{ channel: Channel }>(`/api/v1/communities/${communityId}/channels/${channel.id}`, {
        method: 'PATCH',
        body: JSON.stringify({ name: channel.name, description: channel.description, read_only: channel.read_only, position: channel.position, ...changes }),
      })
      setChannels((current) => current.map((item) => item.id === channel.id ? updated.channel : item))
    } catch (cause) { onError(errorMessage(cause, failure)) }
  }

  async function rename(channel: Channel) {
    const name = window.prompt('Channel name', channel.name)
    if (name === null) return
    await update(channel, { name }, 'Could not rename channel')
  }

  async function remove(channel: Channel) {
    if (!window.confirm(`Delete #${channel.name} and its message history?`)) return
    try {
      await api(`/api/v1/communities/${communityId}/channels/${channel.id}`, { method: 'DELETE' })
      setChannels((current) => current.filter((item) => item.id !== channel.id))
      onChannelDeleted(channel.conversation_id)
    } catch (cause) { onError(errorMessage(cause, 'Could not delete channel')) }
  }

  return (
    <section className="manager-section">
      <h3>Channels</h3>
      {channels.length === 0 ? <p className="muted">No channels yet.</p> : channels.map((channel) => (
        <div className="manager-row" key={channel.id}>
          <span>#{channel.name}</span>
          <select aria-label={`Category for ${channel.name}`} value={channel.category_id ?? ''} onChange={(event) => void update(channel, { category_id: event.target.value || null }, 'Could not move channel')}>
            <option value="">Uncategorized</option>
            {categories.map((category) => <option key={category.id} value={category.id}>{category.name}</option>)}
          </select>
          <button type="button" onClick={() => void rename(channel)}>Rename</button>
          <button type="button" onClick={() => void remove(channel)}>Delete</button>
        </div>
      ))}
    </section>
  )
}

export function MembersSection({ communityId, ownerId, currentUserId, members, setMembers, selectedMemberId, onMemberRemoved, onError }: ErrorProps & {
  communityId: string
  ownerId: string | undefined
  currentUserId: string
  members: User[]
  setMembers: Dispatch<SetStateAction<User[]>>
  selectedMemberId: string
  onMemberRemoved: (memberId: string) => void
}) {
  async function remove(member: User) {
    if (!window.confirm(`Remove ${member.username} from this community?`)) return
    try {
      await api(`/api/v1/communities/${communityId}/members/${member.id}`, { method: 'DELETE' })
      setMembers((current) => current.filter((item) => item.id !== member.id))
      if (selectedMemberId === member.id) onMemberRemoved(member.id)
    } catch (cause) { onError(errorMessage(cause, 'Could not remove member')) }
  }

  return (
    <section className="manager-section">
      <h3>Members</h3>
      {members.map((member) => (
        <div className="manager-row" key={member.id}>
          <span>{member.username}{member.id === ownerId ? ' · owner' : ''}</span>
          {member.id !== currentUserId && member.id !== ownerId && <button type="button" onClick={() => void remove(member)}>Remove</button>}
        </div>
      ))}
    </section>
  )
}
