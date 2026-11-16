import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from 'react'
import { createPortal } from 'react-dom'

const FOCUSABLE = 'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])'

type DialogProps = {
  titleId: string
  closeLabel?: string
  onClose: () => void
  /** Close when the backdrop (outside the dialog) is pressed. */
  closeOnBackdrop?: boolean
  className?: string
  children: ReactNode
}

/**
 * Modal dialog: moves focus in on open, keeps Tab inside, closes on Escape and
 * restores focus to the element that opened it.
 */
export function Dialog({ titleId, closeLabel = 'Close', onClose, closeOnBackdrop = false, className = '', children }: DialogProps) {
  const dialogRef = useRef<HTMLElement | null>(null)
  // Captured while rendering, before React applies `autofocus` inside the dialog, so focus can return to the opener.
  const [opener] = useState(() => document.activeElement)
  const onCloseRef = useRef(onClose)
  useLayoutEffect(() => { onCloseRef.current = onClose })

  useLayoutEffect(() => {
    const dialog = dialogRef.current
    if (!dialog) return
    const previousFocus = opener
    const getFocusable = () => Array.from(dialog.querySelectorAll<HTMLElement>(FOCUSABLE))
      .filter((element) => element.getAttribute('aria-hidden') !== 'true')
    const initialFocus = dialog.querySelector<HTMLElement>('[data-autofocus], [autofocus]')
      ?? dialog.querySelector<HTMLElement>('input:not([disabled]), textarea:not([disabled]), select:not([disabled])')
      ?? getFocusable()[0]
    initialFocus?.focus()
    function keepFocusInside(event: KeyboardEvent) {
      if (event.key !== 'Tab') return
      const elements = getFocusable()
      const first = elements[0]
      const last = elements.at(-1)
      if (!first || !last) { event.preventDefault(); return }
      if (event.shiftKey && (document.activeElement === first || !dialog!.contains(document.activeElement))) {
        event.preventDefault(); last.focus()
      } else if (!event.shiftKey && (document.activeElement === last || !dialog!.contains(document.activeElement))) {
        event.preventDefault(); first.focus()
      }
    }
    document.addEventListener('keydown', keepFocusInside)
    return () => {
      document.removeEventListener('keydown', keepFocusInside)
      if (previousFocus instanceof HTMLElement && previousFocus.isConnected) previousFocus.focus()
    }
  }, [opener])

  useEffect(() => {
    function closeOnEscape(event: KeyboardEvent) {
      if (event.key === 'Escape') onCloseRef.current()
    }
    window.addEventListener('keydown', closeOnEscape)
    return () => window.removeEventListener('keydown', closeOnEscape)
  }, [])

  // Rendered in a portal so a hidden or clipped ancestor (for example the mobile sidebar) cannot hide it.
  return createPortal(
    <div
      className="modal-backdrop"
      onMouseDown={closeOnBackdrop ? (event) => { if (event.target === event.currentTarget) onClose() } : undefined}
    >
      <section ref={dialogRef} className={`dialog ${className}`.trim()} role="dialog" aria-modal="true" aria-labelledby={titleId}>
        <button className="dialog-close" onClick={onClose} aria-label={closeLabel}>×</button>
        {children}
      </section>
    </div>,
    document.body,
  )
}
