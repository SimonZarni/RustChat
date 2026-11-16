import { useEffect, type RefObject } from 'react'

/**
 * Makes the `<details>` panels inside `container` behave like one group of popovers:
 * opening one closes the others, clicking or tabbing elsewhere closes them, and
 * Escape closes the open one and returns focus to its summary.
 */
export function usePanelGroup(container: RefObject<HTMLElement | null>) {
  useEffect(() => {
    const root = container.current
    if (!root) return
    const openPanels = () => Array.from(root.querySelectorAll<HTMLDetailsElement>('details[open]'))

    // Watch the `open` attribute instead of the `toggle` event: toggle events are queued
    // one task per panel, so a late event for an earlier panel could close the one opened
    // after it. Mutation records arrive in the order the panels actually opened.
    const observer = new MutationObserver((records) => {
      const opened = records.filter((record) => record.target instanceof HTMLDetailsElement && record.target.hasAttribute('open') && record.oldValue === null)
      const winner = opened[opened.length - 1]?.target
      if (!(winner instanceof HTMLDetailsElement) || !winner.open) return
      for (const other of openPanels()) if (other !== winner) other.open = false
    })
    const onPointerDown = (event: PointerEvent) => {
      const target = event.target as Node | null
      for (const panel of openPanels()) if (!target || !panel.contains(target)) panel.open = false
    }
    const onFocusIn = (event: FocusEvent) => {
      const target = event.target as Node | null
      if (!target || root.contains(target)) return
      for (const panel of openPanels()) panel.open = false
    }
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return
      const [panel] = openPanels().filter((item) => item.contains(document.activeElement))
      if (!panel) return
      panel.open = false
      panel.querySelector<HTMLElement>('summary')?.focus()
    }

    observer.observe(root, { attributes: true, attributeFilter: ['open'], attributeOldValue: true, subtree: true })
    root.addEventListener('keydown', onKeyDown)
    document.addEventListener('pointerdown', onPointerDown)
    document.addEventListener('focusin', onFocusIn)
    return () => {
      observer.disconnect()
      root.removeEventListener('keydown', onKeyDown)
      document.removeEventListener('pointerdown', onPointerDown)
      document.removeEventListener('focusin', onFocusIn)
    }
  }, [container])
}
