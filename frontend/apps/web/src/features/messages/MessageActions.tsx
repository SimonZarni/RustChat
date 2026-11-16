import { useEffect, useRef, useState, type KeyboardEvent, type ReactNode } from 'react'
import type { Message, MessageReaction } from '../../types'
import { ReactButton } from './ReactionControls'

const Icon = ({ children }: { children: ReactNode }) => (
  <svg viewBox="0 0 20 20" width="16" height="16" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" focusable="false">{children}</svg>
)
const SmileIcon = () => <Icon><circle cx="10" cy="10" r="7.2" /><path d="M7 11.6c.8 1.2 1.7 1.8 3 1.8s2.2-.6 3-1.8" /><path d="M7.6 8.1h.01M12.4 8.1h.01" /></Icon>
const ReplyIcon = () => <Icon><path d="M8.5 5 4 9.5 8.5 14" /><path d="M4.5 9.5H12a4 4 0 0 1 4 4V15" /></Icon>
const MoreIcon = () => <Icon><circle cx="4.5" cy="10" r=".9" fill="currentColor" /><circle cx="10" cy="10" r=".9" fill="currentColor" /><circle cx="15.5" cy="10" r=".9" fill="currentColor" /></Icon>

type Props = {
  message: Message
  own: boolean
  reactions: MessageReaction[] | undefined
  pendingReactions: Record<string, boolean>
  onToggleReaction: (message: Message, emoji: string) => void
  onReply: (message: Message) => void
  onEdit: (message: Message) => void
  onDelete: (message: Message) => void
}

/**
 * Per-message actions: React, Reply and, on your own messages, a More menu for Edit and Delete.
 * The group stays in the DOM (so keyboard and screen-reader users always reach it) and is only
 * made visible while the message is hovered, focused or last touched.
 */
export function MessageActions({ message, own, reactions, pendingReactions, onToggleReaction, onReply, onEdit, onDelete }: Props) {
  return (
    <div className="message-toolbar" role="group" aria-label="Message actions">
      <ReactButton className="toolbar-button" message={message} reactions={reactions} pending={pendingReactions} onToggle={onToggleReaction}><SmileIcon /></ReactButton>
      <button type="button" className="toolbar-button" aria-label="Reply" title="Reply" onClick={() => onReply(message)}><ReplyIcon /></button>
      {own && <MoreMenu onEdit={() => onEdit(message)} onDelete={() => onDelete(message)} />}
    </div>
  )
}

function MoreMenu({ onEdit, onDelete }: { onEdit: () => void; onDelete: () => void }) {
  const [open, setOpen] = useState(false)
  const wrapper = useRef<HTMLSpanElement | null>(null)
  const toggle = useRef<HTMLButtonElement | null>(null)
  const menu = useRef<HTMLDivElement | null>(null)

  useEffect(() => {
    if (!open) return
    menu.current?.querySelector<HTMLElement>('[role="menuitem"]')?.focus()
    const close = (event: Event) => { if (!wrapper.current?.contains(event.target as Node)) setOpen(false) }
    document.addEventListener('pointerdown', close)
    document.addEventListener('focusin', close)
    return () => {
      document.removeEventListener('pointerdown', close)
      document.removeEventListener('focusin', close)
    }
  }, [open])

  function onKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    const list = Array.from(menu.current?.querySelectorAll<HTMLElement>('[role="menuitem"]') ?? [])
    const index = list.indexOf(document.activeElement as HTMLElement)
    if (event.key === 'Escape') { event.preventDefault(); setOpen(false); toggle.current?.focus() }
    else if (event.key === 'Tab') setOpen(false)
    else if (event.key === 'ArrowDown') { event.preventDefault(); list[(index + 1) % list.length]?.focus() }
    else if (event.key === 'ArrowUp') { event.preventDefault(); list[(index - 1 + list.length) % list.length]?.focus() }
    else if (event.key === 'Home') { event.preventDefault(); list[0]?.focus() }
    else if (event.key === 'End') { event.preventDefault(); list[list.length - 1]?.focus() }
  }
  // Return focus to the menu button first, so a dialog opened by the action hands focus back to it.
  const choose = (action: () => void) => () => { setOpen(false); toggle.current?.focus(); action() }

  return (
    <span className="react-wrapper" ref={wrapper}>
      <button type="button" className="toolbar-button" ref={toggle} aria-label="More actions" title="More actions" aria-haspopup="menu" aria-expanded={open} onClick={() => setOpen((current) => !current)}><MoreIcon /></button>
      {open && (
        <div className="message-menu" role="menu" aria-label="More message actions" ref={menu} onKeyDown={onKeyDown}>
          <button type="button" role="menuitem" onClick={choose(onEdit)}>Edit</button>
          <button type="button" role="menuitem" className="danger" onClick={choose(onDelete)}>Delete</button>
        </div>
      )}
    </span>
  )
}
