import { useState } from 'react'
import { api, errorMessage } from '../../lib/api'
import type { AuditEvent, AuditPage } from '../../types'
import { describeAuditEvent } from './describe'

type Props = {
  /** Audit endpoint without query string, for example `/api/v1/admin/audit`. */
  path: string
  title: string
  onError: (message: string) => void
}

/** Newest-first audit history, loaded on demand and paged with a cursor. */
export function AuditLogPanel({ path, title, onError }: Props) {
  const [events, setEvents] = useState<AuditEvent[] | null>(null)
  const [cursor, setCursor] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [denied, setDenied] = useState(false)

  async function load(before: string | null) {
    setBusy(true)
    try {
      const query = before ? `?before=${encodeURIComponent(before)}&limit=25` : '?limit=25'
      const page = await api<AuditPage>(`${path}${query}`)
      setEvents((current) => before ? [...(current ?? []), ...page.events] : page.events)
      setCursor(page.next_cursor)
    } catch (cause) {
      const message = errorMessage(cause, 'Could not load the audit log')
      if (/not found|forbidden|permission/i.test(message)) setDenied(true)
      else onError(message)
    } finally {
      setBusy(false)
    }
  }

  return (
    <section className="manager-section">
      <h3>{title}</h3>
      {denied && <p className="muted">You do not have access to this history.</p>}
      {!denied && events === null && <button type="button" className="outline-button" disabled={busy} onClick={() => void load(null)}>{busy ? 'Loading…' : 'Show history'}</button>}
      {events !== null && (events.length === 0
        ? <p className="muted">Nothing has been recorded yet.</p>
        : <ol className="audit-list" aria-label={title}>
          {events.map((event) => (
            <li className="manager-row" key={event.id}>
              <span>{describeAuditEvent(event)}</span>
              <time dateTime={event.created_at}>{new Date(event.created_at).toLocaleString()}</time>
            </li>
          ))}
        </ol>)}
      {cursor && <button type="button" className="outline-button" disabled={busy} onClick={() => void load(cursor)}>{busy ? 'Loading…' : 'Load more'}</button>}
    </section>
  )
}
