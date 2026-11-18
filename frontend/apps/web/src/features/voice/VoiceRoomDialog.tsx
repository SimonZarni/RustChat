import { useEffect, useRef, useState } from 'react'
import { Participant, Room, RoomEvent, Track } from 'livekit-client'
import { Dialog } from '../../components/Dialog'
import { api, errorMessage } from '../../lib/api'

type VoiceToken = { url: string; token: string; expires_at: number }
export type VoiceTarget = { title: string; tokenPath: string }

export function VoiceRoomDialog({ target, onClose, onError }: { target: VoiceTarget; onClose: () => void; onError: (message: string) => void }) {
  const [room, setRoom] = useState<Room | null>(null)
  const [participants, setParticipants] = useState<Participant[]>([])
  const [devices, setDevices] = useState<MediaDeviceInfo[]>([])
  const [outputs, setOutputs] = useState<MediaDeviceInfo[]>([])
  const [activeDevice, setActiveDevice] = useState('')
  const [activeOutput, setActiveOutput] = useState('')
  const [speakerVolume, setSpeakerVolume] = useState(1)
  const [muted, setMuted] = useState(true)
  const [connecting, setConnecting] = useState(false)
  const [connectionError, setConnectionError] = useState('')
  const [connectionStatus, setConnectionStatus] = useState('Connected')
  const [connectionQuality, setConnectionQuality] = useState('Unknown')
  const audioRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (!room) return
    const syncParticipants = () => setParticipants([room.localParticipant, ...room.remoteParticipants.values()])
    const onTrackSubscribed = (track: Parameters<RoomEventCallbacks['trackSubscribed']>[0]) => {
      if (track.kind !== Track.Kind.Audio || !audioRef.current) return
      const element = track.attach()
      element.autoplay = true
      element.volume = speakerVolume
      audioRef.current.append(element)
    }
    const onTrackUnsubscribed = (track: Parameters<RoomEventCallbacks['trackUnsubscribed']>[0]) => {
      track.detach().forEach((element) => element.remove())
    }
    const onReconnecting = () => setConnectionStatus('Reconnecting…')
    const onReconnected = () => { setConnectionStatus('Connected'); setConnectionQuality(String(room.localParticipant.connectionQuality)) }
    const onDisconnected = () => setConnectionStatus('Disconnected')
    const onQualityChanged = (_quality: unknown, participant: Participant) => {
      if (participant === room.localParticipant) setConnectionQuality(String(participant.connectionQuality))
    }
    room.on(RoomEvent.ParticipantConnected, syncParticipants)
    room.on(RoomEvent.ParticipantDisconnected, syncParticipants)
    room.on(RoomEvent.ActiveSpeakersChanged, syncParticipants)
    room.on(RoomEvent.TrackSubscribed, onTrackSubscribed)
    room.on(RoomEvent.TrackUnsubscribed, onTrackUnsubscribed)
    room.on(RoomEvent.Reconnecting, onReconnecting)
    room.on(RoomEvent.Reconnected, onReconnected)
    room.on(RoomEvent.Disconnected, onDisconnected)
    room.on(RoomEvent.ConnectionQualityChanged, onQualityChanged)
    syncParticipants()
    return () => {
      room.off(RoomEvent.ParticipantConnected, syncParticipants)
      room.off(RoomEvent.ParticipantDisconnected, syncParticipants)
      room.off(RoomEvent.ActiveSpeakersChanged, syncParticipants)
      room.off(RoomEvent.TrackSubscribed, onTrackSubscribed)
      room.off(RoomEvent.TrackUnsubscribed, onTrackUnsubscribed)
      room.off(RoomEvent.Reconnecting, onReconnecting)
      room.off(RoomEvent.Reconnected, onReconnected)
      room.off(RoomEvent.Disconnected, onDisconnected)
      room.off(RoomEvent.ConnectionQualityChanged, onQualityChanged)
    }
  }, [room, speakerVolume])

  useEffect(() => () => { room?.disconnect() }, [room])

  async function connect() {
    if (connecting || room) return
    setConnecting(true)
    setConnectionError('')
    let nextRoom: Room | null = null
    try {
      const credentials = await api<VoiceToken>(target.tokenPath, { method: 'POST' })
      nextRoom = new Room({ adaptiveStream: true, dynacast: true })
      await nextRoom.connect(credentials.url, credentials.token)
      setRoom(nextRoom)
      setConnectionStatus('Connected')
      setConnectionQuality(String(nextRoom.localParticipant.connectionQuality))
      const inputs = await navigator.mediaDevices.enumerateDevices()
      setDevices(inputs.filter((device) => device.kind === 'audioinput'))
      setOutputs(inputs.filter((device) => device.kind === 'audiooutput'))
      setActiveDevice(nextRoom.getActiveDevice('audioinput') ?? '')
      setActiveOutput(nextRoom.getActiveDevice('audiooutput') ?? '')
    } catch (cause) {
      nextRoom?.disconnect()
      const message = errorMessage(cause, 'Could not join voice')
      setConnectionError(message)
      onError(message)
    } finally { setConnecting(false) }
  }

  async function toggleMicrophone() {
    if (!room) return
    try {
      await room.localParticipant.setMicrophoneEnabled(muted)
      setMuted(!muted)
      const inputs = await navigator.mediaDevices.enumerateDevices()
      setDevices(inputs.filter((device) => device.kind === 'audioinput'))
      setOutputs(inputs.filter((device) => device.kind === 'audiooutput'))
      setActiveDevice(room.getActiveDevice('audioinput') ?? '')
    } catch (cause) { setConnectionError(errorMessage(cause, 'Could not change microphone state')) }
  }

  async function chooseOutput(deviceId: string) {
    if (!room) return
    try {
      await room.switchActiveDevice('audiooutput', deviceId)
      setActiveOutput(deviceId)
    } catch (cause) { setConnectionError(errorMessage(cause, 'Could not select speaker')) }
  }

  function setOutputVolume(value: number) {
    setSpeakerVolume(value)
    audioRef.current?.querySelectorAll('audio').forEach((element) => { element.volume = value })
  }

  async function pushToTalk(pressed: boolean) {
    if (!room) return
    try {
      await room.localParticipant.setMicrophoneEnabled(pressed)
      setMuted(!pressed)
    } catch (cause) { setConnectionError(errorMessage(cause, 'Could not enable push to talk')) }
  }

  async function chooseDevice(deviceId: string) {
    if (!room) return
    try {
      await room.switchActiveDevice('audioinput', deviceId)
      setActiveDevice(deviceId)
    } catch (cause) { setConnectionError(errorMessage(cause, 'Could not select microphone')) }
  }

  function close() {
    room?.disconnect()
    setRoom(null)
    onClose()
  }

  return (
    <Dialog titleId="voice-room-title" onClose={close}>
      <div className="voice-room">
        <p className="eyebrow">VOICE CHANNEL</p>
        <h2 id="voice-room-title">{target.title}</h2>
        <p className="muted">{room ? `${connectionStatus} · Connection quality: ${connectionQuality.toLowerCase()}` : 'Join with your microphone muted. You can unmute when you are ready.'}</p>
        {!room ? (
          <button className="primary-button" disabled={connecting} onClick={() => void connect()}>{connecting ? 'Connecting…' : 'Join voice'}</button>
        ) : (
          <>
            <div className="voice-participants" aria-live="polite">
              {participants.map((participant) => <div className={'voice-participant ' + (participant.isSpeaking ? 'speaking' : '')} key={participant.identity}>
                <span className="voice-avatar">{(participant.name || participant.identity).slice(0, 1).toUpperCase()}</span>
                <span>{participant.name || participant.identity}{participant === room.localParticipant ? ' (you)' : ''}</span>
                <span className="voice-speaking">{participant.isSpeaking ? 'Speaking' : participant.isMicrophoneEnabled ? 'Mic on' : 'Muted'}</span>
              </div>)}
              {participants.length === 1 && <p className="muted">You are the first person here.</p>}
            </div>
            {devices.length > 0 && <label className="voice-device">Microphone
              <select value={activeDevice} onChange={(event) => void chooseDevice(event.target.value)}>
                {devices.map((device, index) => <option key={device.deviceId || index} value={device.deviceId}>{device.label || `Microphone ${index + 1}`}</option>)}
              </select>
            </label>}
            {outputs.length > 0 && <label className="voice-device">Speaker
              <select value={activeOutput} onChange={(event) => void chooseOutput(event.target.value)}>
                {outputs.map((device, index) => <option key={device.deviceId || index} value={device.deviceId}>{device.label || `Speaker ${index + 1}`}</option>)}
              </select>
            </label>}
            <label className="voice-device">Speaker volume
              <input aria-label="Speaker volume" type="range" min="0" max="1" step="0.05" value={speakerVolume} onChange={(event) => setOutputVolume(Number(event.target.value))} />
            </label>
            <div className="voice-controls">
              <button className="outline-button" onClick={() => void toggleMicrophone()}>{muted ? 'Unmute microphone' : 'Mute microphone'}</button>
              <button className="outline-button" onPointerDown={(event) => { event.preventDefault(); void pushToTalk(true) }} onPointerUp={() => void pushToTalk(false)} onPointerLeave={() => void pushToTalk(false)} onKeyDown={(event) => { if (event.key === ' ' || event.key === 'Enter') { event.preventDefault(); void pushToTalk(true) } }} onKeyUp={(event) => { if (event.key === ' ' || event.key === 'Enter') void pushToTalk(false) }}>Hold to talk</button>
              <button className="voice-leave-button" onClick={close}>Leave voice</button>
            </div>
          </>
        )}
        {connectionError && <p className="inline-error" role="alert">{connectionError}</p>}
        <div className="voice-audio-output" ref={audioRef} aria-hidden="true" />
      </div>
    </Dialog>
  )
}

type RoomEventCallbacks = {
  trackSubscribed: (track: import('livekit-client').Track, publication: import('livekit-client').TrackPublication, participant: Participant) => void
  trackUnsubscribed: (track: import('livekit-client').Track, publication: import('livekit-client').TrackPublication, participant: Participant) => void
}
