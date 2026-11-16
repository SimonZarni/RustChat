import { lazy, Suspense, useState } from 'react'
import { api, errorMessage } from '../../lib/api'
import type { Community, User } from '../../types'
// The management dialog is only needed by people who open it, so it loads on demand.
const CommunityManagerDialog = lazy(() => import('./CommunityManagerDialog').then((module) => ({ default: module.CommunityManagerDialog })))
import { CreateChannelDialog, CreateCommunityDialog, JoinCommunityDialog } from './CommunityDialogs'
import type { CommunityState } from './useCommunities'

type Dialogs = 'create' | 'join' | 'channel' | 'manage' | null

type Props = {
  state: CommunityState
  user: User
  activeId: string | null
  onOpenConversation: (conversationId: string) => void
  /** The active conversation is no longer valid (community changed, left or deleted). */
  onClearActive: () => void
  onChannelDeleted: (conversationId: string) => void
  onError: (message: string) => void
}

/** Sidebar block listing communities and channels, plus the dialogs that manage them. */
export function CommunitySection({ state, user, activeId, onOpenConversation, onClearActive, onChannelDeleted, onError }: Props) {
  const { communities, selectedCommunityId, channels, categories } = state
  const [dialog, setDialog] = useState<Dialogs>(null)
  const [createdInviteCode, setCreatedInviteCode] = useState('')
  const [inviteCopied, setInviteCopied] = useState(false)
  const selected = communities.find((item) => item.id === selectedCommunityId)
  const selectedChannels = channels.filter((channel) => channel.community_id === selectedCommunityId)
  const channelGroups = [
    ...categories.map((category) => ({ id: category.id, label: category.name, channels: selectedChannels.filter((channel) => channel.category_id === category.id) })),
    { id: 'uncategorized', label: 'UNCATEGORIZED', channels: selectedChannels.filter((channel) => !channel.category_id) },
  ]

  function selectAfterRemoval(removedId: string) {
    const remaining = communities.filter((item) => item.id !== removedId)
    state.setCommunities(remaining)
    state.setSelectedCommunityId(remaining[0]?.id ?? null)
    state.setChannels([])
    onClearActive()
  }

  async function createInvite() {
    if (!selectedCommunityId) return
    try {
      const result = await api<{ invite_code: string }>(`/api/v1/communities/${selectedCommunityId}/invites`, { method: 'POST', body: JSON.stringify({ ttl_seconds: 604800, max_uses: 100 }) })
      setCreatedInviteCode(result.invite_code)
      setInviteCopied(false)
    } catch (cause) { onError(errorMessage(cause, 'Could not create invite')) }
  }

  async function revokeCreatedInvite() {
    if (!selectedCommunityId || !createdInviteCode) return
    try {
      await api(`/api/v1/communities/${selectedCommunityId}/invites`, { method: 'DELETE', body: JSON.stringify({ invite_code: createdInviteCode }) })
      setCreatedInviteCode('')
      setInviteCopied(false)
    } catch (cause) { onError(errorMessage(cause, 'Could not revoke invite')) }
  }

  async function leave() {
    if (!selectedCommunityId || !window.confirm('Leave this community?')) return
    try {
      await api(`/api/v1/communities/${selectedCommunityId}/members`, { method: 'DELETE' })
      selectAfterRemoval(selectedCommunityId)
    } catch (cause) { onError(errorMessage(cause, 'Could not leave community')) }
  }

  function remember(community: Community) {
    state.setCommunities((current) => [...current.filter((item) => item.id !== community.id), community])
    state.setSelectedCommunityId(community.id)
  }

  return (
    <div className="community-area">
      <div className="section-heading">
        <span>COMMUNITIES</span>
        <button onClick={() => setDialog('create')} aria-label="Create community" title="Create community">+</button>
        <button className="join-community-button" onClick={() => setDialog('join')} aria-label="Join community" title="Join community">Join</button>
      </div>
      {communities.map((community) => (
        <button key={community.id} className={'community-item ' + (community.id === selectedCommunityId ? 'selected' : '')} onClick={() => { state.setSelectedCommunityId(community.id); onClearActive() }}>
          <span className="community-avatar">{community.name.slice(0, 1).toUpperCase()}</span>
          <span>{community.name}</span>
          <small>{community.member_count}</small>
        </button>
      ))}
      {selectedCommunityId && (
        <div className="community-actions">
          <button onClick={() => void createInvite()}>Create invite</button>
          <button onClick={() => setDialog('manage')}>Manage</button>
          <button onClick={() => void leave()}>Leave</button>
        </div>
      )}
      {createdInviteCode && (
        <div className="invite-code" role="status">
          <span>Invite code: {createdInviteCode}</span>
          <button onClick={() => navigator.clipboard.writeText(createdInviteCode).then(() => setInviteCopied(true))}>Copy</button>
          {inviteCopied && <small role="status">Copied</small>}
          <button onClick={() => void revokeCreatedInvite()}>Revoke</button>
        </div>
      )}
      {selectedCommunityId && (
        <>
          <div className="section-heading channels-heading">
            <span>CHANNELS</span>
            <button onClick={() => setDialog('channel')} aria-label="Create channel" title="Create channel">+</button>
          </div>
          {channelGroups.filter((group) => group.channels.length > 0).map((group) => (
            <div className="channel-group" key={group.id}>
              <span className="channel-group-label">{group.label}</span>
              {group.channels.map((channel) => (
                <button key={channel.id} className={'channel-item ' + (activeId === channel.conversation_id ? 'selected' : '')} onClick={() => onOpenConversation(channel.conversation_id)}>
                  <span>#</span>{channel.name}
                </button>
              ))}
            </div>
          ))}
        </>
      )}

      {dialog === 'create' && <CreateCommunityDialog onClose={() => setDialog(null)} onError={onError} onCreated={(community) => { state.setCommunities((current) => [...current, community]); state.setSelectedCommunityId(community.id) }} />}
      {dialog === 'join' && <JoinCommunityDialog onClose={() => setDialog(null)} onError={onError} onJoined={remember} />}
      {dialog === 'channel' && selectedCommunityId && (
        <CreateChannelDialog
          communityId={selectedCommunityId} categories={categories} onClose={() => setDialog(null)} onError={onError}
          onCreated={(channel) => { state.setChannels((current) => [...current, channel].sort((a, b) => a.position - b.position)); onOpenConversation(channel.conversation_id) }}
        />
      )}
      {dialog === 'manage' && selected && (
        <Suspense fallback={null}>
        <CommunityManagerDialog
          community={selected} user={user} channels={selectedChannels} categories={categories}
          setChannels={state.setChannels} setCategories={state.setCategories} membersVersion={state.membersVersion}
          onClose={() => setDialog(null)} onError={onError}
          onCommunityUpdated={(community) => state.setCommunities((current) => current.map((item) => item.id === community.id ? community : item))}
          onCommunityDeleted={() => { selectAfterRemoval(selected.id); setDialog(null) }}
          onChannelDeleted={onChannelDeleted}
          onInviteRevoked={() => setCreatedInviteCode('')}
        />
        </Suspense>
      )}
    </div>
  )
}
