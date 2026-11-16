import { useState, type FormEvent } from 'react'
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
  onSend: (content: string) => Promise<void>
  onTyping: (active: boolean) => void
}

/** Per-conversation composer. Mount it with a `key` per conversation so each keeps its own unsent draft. */
export function MessageComposer({ userId, conversationId, placeholder, readOnly, replyTo, replyAuthor, onCancelReply, onSend, onTyping }: Props) {
  const key = draftKey(userId, conversationId)
  const [draft, setDraft] = useState(() => readDraft(key))

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const content = draft.trim()
    if (!content) return
    setDraft('')
    try {
      await onSend(content)
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
        <span><kbd>Enter</kbd> to send <span className="dot-separator">·</span> <kbd>Shift</kbd> + <kbd>Enter</kbd> for a new line</span>
        <button className="send-button" disabled={!draft.trim() || readOnly} aria-label="Send message">↑</button>
      </div>
    </form>
  )
}
