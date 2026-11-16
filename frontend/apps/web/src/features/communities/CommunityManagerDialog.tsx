import { useEffect, useState, type Dispatch, type FormEvent, type SetStateAction } from 'react'
import { Dialog } from '../../components/Dialog'
import { useDialogs } from '../../components/dialogs-context'
import { api, errorMessage } from '../../lib/api'
import type { Category, Channel, Community, CommunityInvite, CommunityRole, User } from '../../types'
import { AuditLogPanel } from '../audit/AuditLogPanel'
import { CategoriesSection, ChannelsSection, InvitesSection, MembersSection } from './ManagerSections'
import { RolesSection } from './RolesSection'

type Props = {
  community: Community
  user: User
  /** Channels belonging to this community. */
  channels: Channel[]
  categories: Category[]
  setChannels: Dispatch<SetStateAction<Channel[]>>
  setCategories: Dispatch<SetStateAction<Category[]>>
  membersVersion: number
  onClose: () => void
  onError: (message: string) => void
  onCommunityUpdated: (community: Community) => void
  onCommunityDeleted: () => void
  onChannelDeleted: (conversationId: string) => void
  onInviteRevoked: () => void
}

export function CommunityManagerDialog({
  community, user, channels, categories, setChannels, setCategories, membersVersion,
  onClose, onError, onCommunityUpdated, onCommunityDeleted, onChannelDeleted, onInviteRevoked,
}: Props) {
  const communityId = community.id
  const dialogs = useDialogs()
  const [roles, setRoles] = useState<CommunityRole[]>([])
  const [members, setMembers] = useState<User[]>([])
  const [invites, setInvites] = useState<CommunityInvite[]>([])
  const [selectedRoleId, setSelectedRoleId] = useState('')
  const [selectedMemberId, setSelectedMemberId] = useState('')
  const [overrideChannelId, setOverrideChannelId] = useState(channels[0]?.id ?? '')
  const [editName, setEditName] = useState(community.name)
  const [editDescription, setEditDescription] = useState(community.description)
  const isOwner = community.owner_id === user.id

  useEffect(() => {
    let alive = true
    Promise.all([
      api<{ roles: CommunityRole[] }>(`/api/v1/communities/${communityId}/roles`),
      api<{ invites: CommunityInvite[] }>(`/api/v1/communities/${communityId}/invites`),
    ]).then(([rolesResult, invitesResult]) => {
      if (!alive) return
      setRoles(rolesResult.roles)
      setInvites(invitesResult.invites)
      setSelectedRoleId((current) => current || rolesResult.roles[0]?.id || '')
    }).catch((cause: unknown) => { if (alive) onError(errorMessage(cause, 'Could not load community management')) })
    return () => { alive = false }
  }, [communityId, onError])

  // Reloads when the server reports a membership change while the dialog is open.
  useEffect(() => {
    let alive = true
    api<{ members: User[] }>(`/api/v1/communities/${communityId}/members`)
      .then(({ members: loaded }) => {
        if (!alive) return
        setMembers(loaded)
        setSelectedMemberId((current) => current || loaded.find((member) => member.id !== user.id)?.id || '')
      })
      .catch((cause: unknown) => { if (alive) onError(errorMessage(cause, 'Could not load community management')) })
    return () => { alive = false }
  }, [communityId, membersVersion, user.id, onError])

  async function saveSettings(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    try {
      const result = await api<{ community: Community }>(`/api/v1/communities/${communityId}`, { method: 'PATCH', body: JSON.stringify({ name: editName, description: editDescription }) })
      onCommunityUpdated(result.community)
    } catch (cause) { onError(errorMessage(cause, 'Could not update community')) }
  }

  async function transferOwnership() {
    try {
      await api(`/api/v1/communities/${communityId}/ownership`, { method: 'POST', body: JSON.stringify({ new_owner_id: selectedMemberId, confirm: true }) })
      onCommunityUpdated({ ...community, owner_id: selectedMemberId })
    } catch (cause) { onError(errorMessage(cause, 'Could not transfer ownership')) }
  }

  async function deleteCommunity() {
    if (!await dialogs.confirm({ title: `Permanently delete ${editName}?`, detail: 'Its channels and their message history are removed for everyone.', confirmLabel: 'Delete community', danger: true })) return
    try {
      await api(`/api/v1/communities/${communityId}`, { method: 'DELETE', body: JSON.stringify({ confirm: true }) })
      onCommunityDeleted()
    } catch (cause) { onError(errorMessage(cause, 'Could not delete community')) }
  }

  return (
    <Dialog titleId="manage-community-title" closeLabel="Close community management" onClose={onClose} closeOnBackdrop className="community-manager">
      <p className="eyebrow">COMMUNITY SETTINGS</p>
      <h2 id="manage-community-title">Manage {community.name}</h2>
      <InvitesSection communityId={communityId} invites={invites} setInvites={setInvites} onInviteRevoked={onInviteRevoked} onError={onError} />
      <form className="stack-form" onSubmit={saveSettings}>
        <label>Name<input required maxLength={80} value={editName} onChange={(event) => setEditName(event.target.value)} /></label>
        <label>Description<textarea maxLength={280} rows={2} value={editDescription} onChange={(event) => setEditDescription(event.target.value)} /></label>
        <button className="primary-button">Save community</button>
      </form>
      <CategoriesSection communityId={communityId} categories={categories} setCategories={setCategories} setChannels={setChannels} onError={onError} />
      <ChannelsSection communityId={communityId} channels={channels} categories={categories} setChannels={setChannels} onChannelDeleted={onChannelDeleted} onError={onError} />
      <MembersSection communityId={communityId} ownerId={community.owner_id} currentUserId={user.id} members={members} setMembers={setMembers} selectedMemberId={selectedMemberId} onMemberRemoved={() => setSelectedMemberId('')} onError={onError} />
      <RolesSection
        communityId={communityId} currentUserId={user.id} roles={roles} setRoles={setRoles} members={members} channels={channels}
        selectedRoleId={selectedRoleId} setSelectedRoleId={setSelectedRoleId} selectedMemberId={selectedMemberId} setSelectedMemberId={setSelectedMemberId}
        overrideChannelId={overrideChannelId} setOverrideChannelId={setOverrideChannelId} onError={onError}
      />
      <AuditLogPanel path={`/api/v1/communities/${communityId}/audit`} title="Audit history" onError={onError} />
      {isOwner && (
        <section className="manager-section">
          <h3>Ownership</h3>
          <label>Transfer to
            <select value={selectedMemberId} onChange={(event) => setSelectedMemberId(event.target.value)}>
              <option value="">Choose member</option>
              {members.filter((member) => member.id !== user.id).map((member) => <option key={member.id} value={member.id}>{member.username}</option>)}
            </select>
          </label>
          <button className="outline-button" disabled={!selectedMemberId} onClick={() => void transferOwnership()}>Transfer ownership</button>
          <button className="danger-button" onClick={() => void deleteCommunity()}>Delete community</button>
        </section>
      )}
    </Dialog>
  )
}
