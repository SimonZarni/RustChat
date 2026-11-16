import { useState, type FormEvent } from 'react'
import { Dialog } from '../../components/Dialog'
import { api, errorMessage } from '../../lib/api'
import type { Conversation, UserProfile } from '../../types'

type Props = {
  onClose: () => void
  onError: (message: string) => void
  onCreated: (conversation: Conversation) => void
}

export function NewConversationDialog({ onClose, onError, onCreated }: Props) {
  const [kind, setKind] = useState<'direct' | 'group'>('direct')
  const [participants, setParticipants] = useState('')
  const [creating, setCreating] = useState(false)
  const [preview, setPreview] = useState<UserProfile | null>(null)
  const [previewError, setPreviewError] = useState('')
  const [previewBusy, setPreviewBusy] = useState(false)

  async function create(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); setCreating(true); onError('')
    const names = participants.split(/[\s,]+/).map((name) => name.trim()).filter(Boolean)
    try {
      const body = kind === 'direct' ? { type: 'direct', username: names[0] } : { type: 'group', usernames: names }
      const result = await api<{ conversation: Conversation }>('/api/v1/conversations', { method: 'POST', body: JSON.stringify(body) })
      onCreated(result.conversation)
      onClose()
    } catch (cause) { onError(errorMessage(cause, 'Could not create conversation')); setCreating(false) }
  }

  async function previewProfile() {
    const username = participants.trim()
    if (!username || kind !== 'direct') return
    setPreviewBusy(true)
    setPreviewError('')
    try {
      const result = await api<{ profile: UserProfile }>(`/api/v1/users/${encodeURIComponent(username)}/profile`)
      setPreview(result.profile)
    } catch (cause) {
      setPreview(null)
      setPreviewError(errorMessage(cause, 'Profile is not available'))
    } finally {
      setPreviewBusy(false)
    }
  }

  return (
    <Dialog titleId="dialog-title" onClose={onClose} closeOnBackdrop>
      <p className="eyebrow">NEW MESSAGE</p>
      <h2 id="dialog-title">Start a conversation</h2>
      <p className="muted">Find people by their exact username.</p>
      <div className="kind-switch" role="group" aria-label="Conversation type">
        <button className={kind === 'direct' ? 'active' : ''} onClick={() => setKind('direct')}>Direct</button>
        <button className={kind === 'group' ? 'active' : ''} onClick={() => setKind('group')}>Group</button>
      </div>
      <form className="stack-form" onSubmit={create}>
        <label>{kind === 'direct' ? 'Username' : 'Usernames'}
          <input
            autoFocus required value={participants} placeholder={kind === 'direct' ? 'e.g. riverstone' : 'Separate usernames with commas'}
            onChange={(event) => { setParticipants(event.target.value); setPreview(null); setPreviewError('') }}
          />
        </label>
        {kind === 'direct' && <>
          <button type="button" className="outline-button" disabled={!participants.trim() || previewBusy} onClick={() => void previewProfile()}>{previewBusy ? 'Loading profile...' : 'View profile'}</button>
          {preview && <div className="profile-preview"><strong>{preview.display_name || preview.username}</strong><span>@{preview.username}</span>{preview.bio && <p>{preview.bio}</p>}</div>}
          {previewError && <p className="inline-error" role="status">{previewError}</p>}
        </>}
        {kind === 'group' && <span className="field-hint">You’ll be included automatically. Up to 100 people.</span>}
        <button className="primary-button" disabled={creating}>{creating ? 'Creating…' : 'Continue'}</button>
      </form>
    </Dialog>
  )
}
