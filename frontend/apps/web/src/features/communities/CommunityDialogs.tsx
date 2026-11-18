import { useState, type FormEvent } from 'react'
import { Dialog } from '../../components/Dialog'
import { api, errorMessage } from '../../lib/api'
import type { Category, Channel, Community } from '../../types'

type DialogProps = { onClose: () => void; onError: (message: string) => void }

export function CreateCommunityDialog({ onClose, onError, onCreated }: DialogProps & { onCreated: (community: Community) => void }) {
  const [name, setName] = useState('')

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    try {
      const result = await api<{ community: Community }>('/api/v1/communities', { method: 'POST', body: JSON.stringify({ name }) })
      onCreated(result.community)
      onClose()
    } catch (cause) { onError(errorMessage(cause, 'Could not create community')) }
  }

  return (
    <Dialog titleId="community-title" onClose={onClose}>
      <p className="eyebrow">NEW COMMUNITY</p>
      <h2 id="community-title">Create a community</h2>
      <p className="muted">Give your group a name. You can invite people next.</p>
      <form className="stack-form" onSubmit={submit}>
        <label>Community name<input autoFocus required minLength={2} maxLength={80} value={name} onChange={(event) => setName(event.target.value)} /></label>
        <button className="primary-button">Create community</button>
      </form>
    </Dialog>
  )
}

export function JoinCommunityDialog({ onClose, onError, onJoined }: DialogProps & { onJoined: (community: Community) => void }) {
  const [inviteCode, setInviteCode] = useState('')

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    try {
      const result = await api<{ community: Community }>('/api/v1/communities/join', { method: 'POST', body: JSON.stringify({ invite_code: inviteCode.trim() }) })
      onJoined(result.community)
      onClose()
    } catch (cause) { onError(errorMessage(cause, 'Could not join community')) }
  }

  return (
    <Dialog titleId="join-title" onClose={onClose}>
      <p className="eyebrow">JOIN A COMMUNITY</p>
      <h2 id="join-title">Enter an invite code</h2>
      <form className="stack-form" onSubmit={submit}>
        <label>Invite code<input autoFocus required value={inviteCode} onChange={(event) => setInviteCode(event.target.value)} /></label>
        <button className="primary-button">Join community</button>
      </form>
    </Dialog>
  )
}

type CreateChannelProps = DialogProps & {
  communityId: string
  categories: Category[]
  onCreated: (channel: Channel) => void
}

export function CreateChannelDialog({ communityId, categories, onClose, onError, onCreated }: CreateChannelProps) {
  const [name, setName] = useState('')
  const [categoryId, setCategoryId] = useState('')
  const [channelType, setChannelType] = useState<'text' | 'voice'>('text')

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    try {
      const result = await api<{ channel: Channel }>(`/api/v1/communities/${communityId}/channels`, { method: 'POST', body: JSON.stringify({ name, category_id: categoryId || null, channel_type: channelType }) })
      onCreated(result.channel)
      onClose()
    } catch (cause) { onError(errorMessage(cause, 'Could not create channel')) }
  }

  return (
    <Dialog titleId="channel-title" onClose={onClose}>
      <p className="eyebrow">NEW CHANNEL</p>
      <h2 id="channel-title">Create a channel</h2>
      <form className="stack-form" onSubmit={submit}>
        <label>Channel name<input autoFocus required maxLength={64} value={name} onChange={(event) => setName(event.target.value)} placeholder="general" /></label>
        <label>Channel type
          <select value={channelType} onChange={(event) => setChannelType(event.target.value as 'text' | 'voice')}>
            <option value="text">Text</option>
            <option value="voice">Voice</option>
          </select>
        </label>
        <label>Category
          <select value={categoryId} onChange={(event) => setCategoryId(event.target.value)}>
            <option value="">Uncategorized</option>
            {categories.map((category) => <option key={category.id} value={category.id}>{category.name}</option>)}
          </select>
        </label>
        <button className="primary-button">Create {channelType} channel</button>
      </form>
    </Dialog>
  )
}
