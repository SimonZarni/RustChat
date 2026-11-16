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

    // `toggle` does not bubble, so listen during the capture phase.
    const onToggle = (event: Event) => {
      const panel = event.target
      if (!(panel instanceof HTMLDetailsElement) || !panel.open) return
      for (const other of openPanels()) if (other !== panel) other.open = false
    }
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

    root.addEventListener('toggle', onToggle, true)
    root.addEventListener('keydown', onKeyDown)
    document.addEventListener('pointerdown', onPointerDown)
    document.addEventListener('focusin', onFocusIn)
    return () => {
      root.removeEventListener('toggle', onToggle, true)
      root.removeEventListener('keydown', onKeyDown)
      document.removeEventListener('pointerdown', onPointerDown)
      document.removeEventListener('focusin', onFocusIn)
    }
  }, [container])
}
