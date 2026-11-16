import { useEffect, useRef, useState, type ReactNode } from 'react'
import { REACTION_EMOJI } from '../../lib/permissions'
import type { Message, MessageReaction } from '../../types'

type Props = {
  message: Message
  reactions: MessageReaction[] | undefined
  pending: Record<string, boolean>
  onToggle: (message: Message, emoji: string) => void
  /** Style and content of the opening button; defaults to a "React" text link. */
  className?: string
  children?: ReactNode
}

const label = (emoji: string, reaction: MessageReaction | undefined) =>
  `${reaction?.reacted_by_me ? 'Remove' : 'Add'} ${emoji} reaction${reaction ? `, ${reaction.count} total` : ''}`

/**
 * Reactions that exist on a message, shown as chips, plus a "React" button that opens a
 * small picker of the allowed emoji. Nothing is drawn for reactions nobody has used.
 */
export function ReactionChips({ message, reactions, pending, onToggle }: Props) {
  const used = REACTION_EMOJI.filter((emoji) => (reactions?.find((item) => item.emoji === emoji)?.count ?? 0) > 0)
  if (used.length === 0) return null
  return (
    <div className="message-reactions" role="group" aria-label="Reactions">
      {used.map((emoji) => {
        const reaction = reactions?.find((item) => item.emoji === emoji)
        return (
          <button
            key={emoji} type="button" className={reaction?.reacted_by_me ? 'selected' : ''}
            aria-pressed={reaction?.reacted_by_me ?? false} aria-label={label(emoji, reaction)}
            disabled={pending[`${message.id}:${emoji}`]} onClick={() => onToggle(message, emoji)}
          >{emoji}{reaction && <span>{reaction.count}</span>}</button>
        )
      })}
    </div>
  )
}

export function ReactButton({ message, reactions, pending, onToggle, className = 'message-link', children }: Props) {
  const [open, setOpen] = useState(false)
  const wrapper = useRef<HTMLSpanElement | null>(null)
  const toggle = useRef<HTMLButtonElement | null>(null)

  useEffect(() => {
    if (!open) return
    const onPointerDown = (event: PointerEvent) => {
      if (!wrapper.current?.contains(event.target as Node)) setOpen(false)
    }
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return
      setOpen(false)
      toggle.current?.focus()
    }
    const onFocusIn = (event: FocusEvent) => {
      if (!wrapper.current?.contains(event.target as Node)) setOpen(false)
    }
    document.addEventListener('pointerdown', onPointerDown)
    document.addEventListener('keydown', onKeyDown)
    document.addEventListener('focusin', onFocusIn)
    return () => {
      document.removeEventListener('pointerdown', onPointerDown)
      document.removeEventListener('keydown', onKeyDown)
      document.removeEventListener('focusin', onFocusIn)
    }
  }, [open])

  return (
    <span className="react-wrapper" ref={wrapper}>
      <button type="button" className={className} ref={toggle} aria-label="React" title="React" aria-haspopup="true" aria-expanded={open} onClick={() => setOpen((current) => !current)}>{children ?? 'React'}</button>
      {open && (
        <div className="reaction-picker" role="group" aria-label="Choose a reaction">
          {REACTION_EMOJI.map((emoji) => {
            const reaction = reactions?.find((item) => item.emoji === emoji)
            return (
              <button
                key={emoji} type="button" aria-pressed={reaction?.reacted_by_me ?? false} aria-label={label(emoji, reaction)}
                disabled={pending[`${message.id}:${emoji}`]}
                onClick={() => { onToggle(message, emoji); setOpen(false); toggle.current?.focus() }}
              >{emoji}</button>
            )
          })}
        </div>
      )}
    </span>
  )
}
