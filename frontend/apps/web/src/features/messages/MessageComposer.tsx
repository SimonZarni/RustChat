import { useEffect, useRef, useState, type FormEvent } from 'react'
import { api, errorMessage } from '../../lib/api'
import type { Message } from '../../types'

const draftKey = (userId: string, conversationId: string) => `rustchat:draft:${userId}:${conversationId}`

function readDraft(key: string): string {
  try { return localStorage.getItem(key) ?? '' } catch { return '' }
}

function writeDraft(key: string, value: string) {
  try {
    if (value) localStorage.setItem(key, value)
    else localStorage.removeItem(key)
  } catch { /* Drafts are a convenience; storage may be unavailable. */ }
}

type Props = {
  userId: string
  conversationId: string
  placeholder: string
  readOnly: boolean
  replyTo: Message | null
  replyAuthor: string
  onCancelReply: () => void
  /** Rejects when the message could not be sent, so the draft is restored. */
  onSend: (content: string, attachmentIds: string[]) => Promise<void>
  onTyping: (active: boolean) => void
}

/** Per-conversation composer. Mount it with a `key` per conversation so each keeps its own unsent draft. */
export function MessageComposer({ userId, conversationId, placeholder, readOnly, replyTo, replyAuthor, onCancelReply, onSend, onTyping }: Props) {
  const key = draftKey(userId, conversationId)
  const [draft, setDraft] = useState(() => readDraft(key))
  const [uploads, setUploads] = useState<{ localId: string; attachmentId?: string; name: string; size: number; progress: number; status: 'uploading' | 'ready' | 'failed'; error?: string }[]>([])
  const xhrs = useRef(new Map<string, XMLHttpRequest>())
  const attachmentIds = useRef(new Map<string, string>())

  useEffect(() => () => {
    for (const xhr of xhrs.current.values()) xhr.abort()
    for (const id of attachmentIds.current.values()) void api(`/api/v1/attachments/${id}`, { method: 'DELETE' }).catch(() => {})
  }, [])

  async function uploadFile(file: File) {
    const localId = crypto.randomUUID()
    setUploads((current) => [...current, { localId, name: file.name, size: file.size, progress: 0, status: 'uploading' }])
    try {
      const reservation = await api<{ attachment_id: string; upload_url: string; required_content_type: string }>(`/api/v1/conversations/${conversationId}/attachments`, {
        method: 'POST', body: JSON.stringify({ file_name: file.name, content_type: file.type || 'application/octet-stream', byte_size: file.size }),
      })
      attachmentIds.current.set(localId, reservation.attachment_id)
      setUploads((current) => current.map((item) => item.localId === localId ? { ...item, attachmentId: reservation.attachment_id } : item))
      await new Promise<void>((resolve, reject) => {
        const xhr = new XMLHttpRequest()
        xhrs.current.set(localId, xhr)
        xhr.open('PUT', reservation.upload_url)
        xhr.setRequestHeader('Content-Type', reservation.required_content_type)
        xhr.upload.onprogress = (event) => {
          if (event.lengthComputable) setUploads((current) => current.map((item) => item.localId === localId ? { ...item, progress: Math.round(event.loaded * 100 / event.total) } : item))
        }
        xhr.onload = () => xhr.status >= 200 && xhr.status < 300 ? resolve() : reject(new Error(`Upload failed (${xhr.status})`))
        xhr.onerror = () => reject(new Error('Upload failed. Check the storage connection.'))
        xhr.onabort = () => reject(new Error('Upload canceled.'))
        xhr.send(file)
      })
      xhrs.current.delete(localId)
      await api(`/api/v1/attachments/${reservation.attachment_id}/complete`, { method: 'POST' })
      setUploads((current) => current.map((item) => item.localId === localId ? { ...item, status: 'ready', progress: 100 } : item))
    } catch (cause) {
      xhrs.current.delete(localId)
      setUploads((current) => current.some((item) => item.localId === localId) ? current.map((item) => item.localId === localId ? { ...item, status: 'failed', error: errorMessage(cause, 'Upload failed') } : item) : current)
    }
  }

  function cancelUpload(localId: string) {
    xhrs.current.get(localId)?.abort()
    xhrs.current.delete(localId)
    const id = attachmentIds.current.get(localId)
    if (id) void api(`/api/v1/attachments/${id}`, { method: 'DELETE' }).catch(() => {})
    attachmentIds.current.delete(localId)
    setUploads((current) => current.filter((item) => item.localId !== localId))
  }

  function chooseFiles(files: FileList | null) {
    const picked = Array.from(files ?? [])
    if (picked.length + uploads.length > 10) return
    for (const file of picked) void uploadFile(file)
  }

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const content = draft.trim()
    const ready = uploads.filter((item) => item.status === 'ready' && item.attachmentId)
    if (!content && ready.length === 0) return
    setDraft('')
    try {
      await onSend(content, ready.map((item) => item.attachmentId!))
      for (const item of ready) attachmentIds.current.delete(item.localId)
      setUploads((current) => current.filter((item) => item.status !== 'ready'))
      writeDraft(key, '')
    } catch {
      setDraft(content)
      writeDraft(key, content)
    }
  }

  return (
    <form className="message-composer" onSubmit={submit}>
      {replyTo && (
        <div className="replying-to">
          <span>Replying to {replyAuthor}: {replyTo.content}</span>
          <button type="button" onClick={onCancelReply} aria-label="Cancel reply">×</button>
        </div>
      )}
      {uploads.length > 0 && <ul className="attachment-staging" aria-label="Attachments being prepared">
        {uploads.map((item) => <li key={item.localId}>
          <span className="attachment-file-mark" aria-hidden="true">↗</span>
          <span className="attachment-file-copy"><strong>{item.name}</strong><small>{(item.size / 1024 / 1024).toFixed(1)} MB · {item.status === 'ready' ? 'Ready' : item.status === 'failed' ? item.error : `Uploading ${item.progress}%`}</small>
            {item.status === 'uploading' && <progress value={item.progress} max="100" aria-label={`Uploading ${item.name}`} />}
          </span>
          <button type="button" className="attachment-cancel" onClick={() => cancelUpload(item.localId)} aria-label={`Remove ${item.name}`}>×</button>
        </li>)}
      </ul>}
      <textarea
        aria-label="Write a message" placeholder={placeholder} disabled={readOnly} rows={1} maxLength={4000} value={draft}
        onChange={(event) => {
          setDraft(event.target.value)
          writeDraft(key, event.target.value)
          if (event.target.value.trim()) onTyping(true)
        }}
        onBlur={() => onTyping(false)}
        onKeyDown={(event) => { if (event.key === 'Enter' && !event.shiftKey) { event.preventDefault(); event.currentTarget.form?.requestSubmit() } }}
      />
      <div className="composer-footer">
        <span className="composer-hint"><kbd>Enter</kbd> to send <span className="dot-separator">·</span> <kbd>Shift</kbd> + <kbd>Enter</kbd> for a new line</span>
        <div className="composer-actions">
          <label className="attach-button" aria-label="Add attachments">Attach<input type="file" multiple accept="image/png,image/jpeg,image/gif,image/webp,application/pdf,text/plain" disabled={readOnly || uploads.length >= 10} onChange={(event) => { chooseFiles(event.currentTarget.files); event.currentTarget.value = '' }} /></label>
          <button className="send-button" disabled={(!draft.trim() && !uploads.some((item) => item.status === 'ready')) || readOnly || uploads.some((item) => item.status === 'uploading' || item.status === 'failed')} aria-label="Send message">↑</button>
        </div>
      </div>
    </form>
  )
}
