import { useEffect, useState } from 'react'
import { api, errorMessage } from '../../lib/api'

type Person = { id: string; username: string }
type Request = { id: string; user: Person; created_at: string }
type Lists = { friends: Person[]; incoming: Request[]; outgoing: Request[]; blocks: Person[] }

export function PeopleDialog({ onClose, onError }: { onClose: () => void; onError: (message: string) => void }) {
  const [lists, setLists] = useState<Lists>({ friends: [], incoming: [], outgoing: [], blocks: [] })
  const [username, setUsername] = useState('')
  const [busy, setBusy] = useState(false)
  const [notice, setNotice] = useState('')
  async function refresh() {
    const [friends, incoming, outgoing, blocks] = await Promise.all([
      api<{ users: Person[] }>('/api/v1/friends'),
      api<{ requests: Request[] }>('/api/v1/friend-requests?direction=incoming'),
      api<{ requests: Request[] }>('/api/v1/friend-requests?direction=outgoing'),
      api<{ users: Person[] }>('/api/v1/blocks'),
    ])
    setLists({ friends: friends.users, incoming: incoming.requests, outgoing: outgoing.requests, blocks: blocks.users })
  }
  useEffect(() => { void refresh().catch((cause: unknown) => onError(errorMessage(cause, 'Could not load people'))) }, [onError])
  async function act(operation: () => Promise<unknown>, message: string) {
    setBusy(true)
    try { await operation(); await refresh(); setNotice(message) }
    catch (cause) { onError(errorMessage(cause, 'Could not update relationship')) }
    finally { setBusy(false) }
  }
  async function submit(event: React.FormEvent, block: boolean) {
    event.preventDefault(); const value = username.trim(); if (!value) return
    await act(() => api(block ? '/api/v1/blocks' : '/api/v1/friend-requests', { method: 'POST', body: JSON.stringify({ username: value }) }), block ? `Blocked @${value}` : `Request sent to @${value}`)
    setUsername('')
  }
  return <div className="modal-backdrop" onMouseDown={(event) => { if (event.target === event.currentTarget) onClose() }}>
    <section className="dialog people-dialog" role="dialog" aria-modal="true" aria-labelledby="people-title">
      <button className="dialog-close" onClick={onClose} aria-label="Close people">×</button>
      <p className="eyebrow">YOUR NETWORK</p><h2 id="people-title">Friends &amp; blocked</h2>
      <p className="dialog-copy">Manage friend requests and decide who can contact you.</p>
      <form className="people-add" onSubmit={(event) => void submit(event, false)}><input aria-label="Username" value={username} onChange={(event) => setUsername(event.target.value)} placeholder="Find by exact username" maxLength={32} /><button disabled={busy || !username.trim()}>Add friend</button></form>
      <form className="people-block-form" onSubmit={(event) => void submit(event, true)}><button className="text-button" disabled={busy || !username.trim()}>Block this username</button></form>
      {notice && <p className="people-notice" role="status">{notice}</p>}
      <div className="people-lists">
        <PeopleSection title="Incoming requests" empty="No incoming requests" items={lists.incoming.map((item) => ({ person: item.user, id: item.id }))} render={(item) => <><button disabled={busy} onClick={() => void act(() => api(`/api/v1/friend-requests/${item.id}/accept`, { method: 'POST' }), `Added @${item.person.username}`)}>Accept</button><button className="text-button" disabled={busy} onClick={() => void act(() => api(`/api/v1/friend-requests/${item.id}/reject`, { method: 'POST' }), 'Request declined')}>Decline</button></>} />
        <PeopleSection title="Friends" empty="Your friends will appear here" items={lists.friends.map((person) => ({ person, id: person.id }))} render={(item) => <button className="text-button" disabled={busy} onClick={() => void act(() => api(`/api/v1/friends/${item.id}`, { method: 'DELETE' }), `Removed @${item.person.username}`)}>Remove</button>} />
        <PeopleSection title="Outgoing requests" empty="No pending requests" items={lists.outgoing.map((item) => ({ person: item.user, id: item.id }))} render={(item) => <button className="text-button" disabled={busy} onClick={() => void act(() => api(`/api/v1/friend-requests/${item.id}`, { method: 'DELETE' }), 'Request canceled')}>Cancel</button>} />
        <PeopleSection title="Blocked users" empty="No blocked users" items={lists.blocks.map((person) => ({ person, id: person.id }))} render={(item) => <button className="text-button" disabled={busy} onClick={() => void act(() => api(`/api/v1/blocks/${item.id}`, { method: 'DELETE' }), `Unblocked @${item.person.username}`)}>Unblock</button>} />
      </div>
    </section>
  </div>
}

function PeopleSection({ title, empty, items, render }: { title: string; empty: string; items: { id: string; person: Person }[]; render: (item: { id: string; person: Person }) => React.ReactNode }) {
  return <section className="people-section"><h3>{title}</h3>{items.length ? items.map((item) => <div className="people-row" key={item.id}><span>@{item.person.username}</span><div>{render(item)}</div></div>) : <p>{empty}</p>}</section>
}
