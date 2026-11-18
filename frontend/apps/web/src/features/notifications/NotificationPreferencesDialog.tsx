import { useEffect, useState } from 'react'
import { api, errorMessage } from '../../lib/api'

type Target = { id: string; label: string }
type Preferences = { conversation_id: string; messages_enabled: boolean; mentions_enabled: boolean; muted: boolean; muted_until: string | null }
type Dnd = { enabled: boolean; start_time: string; end_time: string; timezone: string; weekdays: number[] }

export function NotificationPreferencesDialog({ targets, communityIds, onClose, onError }: { targets: Target[]; communityIds: string[]; onClose: () => void; onError: (message: string) => void }) {
  const [allTargets, setAllTargets] = useState(targets)
  const [selected, setSelected] = useState(targets[0]?.id ?? '')
  const [value, setValue] = useState<Preferences | null>(null)
  const [muteChoice, setMuteChoice] = useState('off')
  const [saving, setSaving] = useState(false)
  const [dnd, setDnd] = useState<Dnd>({ enabled: false, start_time: '22:00', end_time: '08:00', timezone: Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC', weekdays: [1, 2, 3, 4, 5, 6, 7] })
  const [dndSaving, setDndSaving] = useState(false)
  const [browserEnabled, setBrowserEnabled] = useState(localStorage.getItem('rustchat.browserNotifications') === 'true')
  useEffect(() => { api<Dnd>('/api/v1/notifications/dnd-schedule').then(setDnd).catch((cause: unknown) => onError(errorMessage(cause, 'Could not load DND schedule'))) }, [onError])
  useEffect(() => {
    let alive = true
    Promise.all(communityIds.map(async (id) => {
      const result = await api<{ channels: { conversation_id: string; name: string }[] }>(`/api/v1/communities/${id}/channels`)
      return result.channels.map((channel) => ({ id: channel.conversation_id, label: `#${channel.name}` }))
    })).then((groups) => {
      if (!alive) return
      const combined = [...targets, ...groups.flat()]
      const unique = [...new Map(combined.map((target) => [target.id, target])).values()]
      setAllTargets(unique)
      setSelected((current) => current || unique[0]?.id || '')
    }).catch((cause: unknown) => { if (alive) onError(errorMessage(cause, 'Could not load channel preferences')) })
    return () => { alive = false }
  }, [targets, communityIds, onError])
  useEffect(() => {
    if (!selected) return
    let alive = true
    api<Preferences>(`/api/v1/conversations/${selected}/notification-preferences`)
      .then((result) => { if (alive) { setValue(result); setMuteChoice(result.muted ? 'forever' : result.muted_until && new Date(result.muted_until) > new Date() ? 'timed' : 'off') } })
      .catch((cause: unknown) => onError(errorMessage(cause, 'Could not load notification preferences')))
    return () => { alive = false }
  }, [selected, onError])
  async function save() {
    if (!value) return
    setSaving(true)
    const muted = muteChoice === 'forever'
    const mutedUntil = muteChoice === 'timed' ? new Date(Date.now() + 8 * 60 * 60 * 1000).toISOString() : null
    try {
      const result = await api<Preferences>(`/api/v1/conversations/${selected}/notification-preferences`, { method: 'PUT', body: JSON.stringify({ messages_enabled: value.messages_enabled, mentions_enabled: value.mentions_enabled, muted, muted_until: mutedUntil }) })
      setValue(result)
      onClose()
    } catch (cause) { onError(errorMessage(cause, 'Could not save notification preferences')) }
    finally { setSaving(false) }
  }
  async function saveDnd() {
    setDndSaving(true)
    try { setDnd(await api<Dnd>('/api/v1/notifications/dnd-schedule', { method: 'PUT', body: JSON.stringify(dnd) })) }
    catch (cause) { onError(errorMessage(cause, 'Could not save DND schedule')) }
    finally { setDndSaving(false) }
  }
  async function enableBrowserNotifications() {
    if (!('Notification' in window)) { onError('This browser does not support desktop notifications'); return }
    const result = await Notification.requestPermission()
    const enabled = result === 'granted'
    setBrowserEnabled(enabled)
    localStorage.setItem('rustchat.browserNotifications', String(enabled))
    if (!enabled) onError('Browser notification permission was not granted')
  }
  return <section className="dialog notification-preferences-dialog" role="dialog" aria-modal="true" aria-labelledby="notification-preferences-title">
    <button className="dialog-close" onClick={onClose} aria-label="Close notification settings">×</button>
    <p className="eyebrow">MESSAGES</p><h2 id="notification-preferences-title">Notification settings</h2>
    <section className="dnd-settings"><h3>Do not disturb</h3><label className="notification-pref-check"><span>Use a quiet schedule</span><input type="checkbox" checked={dnd.enabled} onChange={(event) => setDnd({ ...dnd, enabled: event.target.checked })} /></label>
      <div className="dnd-fields"><label>From<input type="time" value={dnd.start_time} onChange={(event) => setDnd({ ...dnd, start_time: event.target.value })} /></label><label>Until<input type="time" value={dnd.end_time} onChange={(event) => setDnd({ ...dnd, end_time: event.target.value })} /></label><label>Time zone<input value={dnd.timezone} onChange={(event) => setDnd({ ...dnd, timezone: event.target.value })} /></label></div>
      <div className="dnd-days">{['Mon','Tue','Wed','Thu','Fri','Sat','Sun'].map((day, index) => <label key={day}><input type="checkbox" checked={dnd.weekdays.includes(index + 1)} onChange={(event) => setDnd({ ...dnd, weekdays: event.target.checked ? [...dnd.weekdays, index + 1].sort() : dnd.weekdays.filter((value) => value !== index + 1) })} />{day}</label>)}</div>
      <div className="dialog-actions"><button disabled={dndSaving} onClick={() => void saveDnd()}>{dndSaving ? 'Saving…' : 'Save quiet hours'}</button></div>
    </section>
    <section className="dnd-settings"><h3>Browser and desktop</h3><p>Show notification alerts while RustChat is open in this browser.</p><button className="secondary-button" onClick={() => void enableBrowserNotifications()}>{browserEnabled ? 'Browser alerts enabled' : 'Enable browser alerts'}</button></section>
    {allTargets.length === 0 ? <p>No conversations or channels yet.</p> : <>
      <label className="field-label">Conversation or channel<select value={selected} onChange={(event) => { setSelected(event.target.value); setValue(null) }}>{allTargets.map((target) => <option key={target.id} value={target.id}>{target.label}</option>)}</select></label>
      {value && <div className="notification-preference-options">
        <label><span>New messages</span><input type="checkbox" checked={value.messages_enabled} onChange={(event) => setValue({ ...value, messages_enabled: event.target.checked })} /></label>
        <label><span>Mentions</span><input type="checkbox" checked={value.mentions_enabled} onChange={(event) => setValue({ ...value, mentions_enabled: event.target.checked })} /></label>
        <label className="field-label">Mute<select value={muteChoice} onChange={(event) => setMuteChoice(event.target.value)}><option value="off">Off</option><option value="timed">For 8 hours</option><option value="forever">Until I turn it back on</option></select></label>
      </div>}
      <div className="dialog-actions"><button className="secondary-button" onClick={onClose}>Cancel</button><button disabled={!value || saving} onClick={() => void save()}>{saving ? 'Saving…' : 'Save settings'}</button></div>
    </>}
  </section>
}
