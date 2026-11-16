import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { useRef } from 'react'
import { afterEach, describe, expect, it } from 'vitest'
import { usePanelGroup } from './usePanelGroup'

afterEach(cleanup)

function Harness() {
  const ref = useRef<HTMLElement | null>(null)
  usePanelGroup(ref)
  return (
    <>
      <footer ref={ref}>
        <details data-testid="a"><summary>First</summary><input aria-label="inside a" /></details>
        <details data-testid="b"><summary>Second</summary><input aria-label="inside b" /></details>
      </footer>
      <button>Elsewhere</button>
    </>
  )
}

const open = (id: string) => {
  const panel = screen.getByTestId(id) as HTMLDetailsElement
  panel.open = true
  fireEvent(panel, new Event('toggle'))
  return panel
}

describe('usePanelGroup', () => {
  it('closes the other panel when one opens', () => {
    render(<Harness />)
    const a = open('a')
    const b = open('b')
    expect(b.open).toBe(true)
    expect(a.open).toBe(false)
  })

  it('closes when the pointer goes down outside, but not inside the panel', () => {
    render(<Harness />)
    const a = open('a')
    fireEvent.pointerDown(screen.getByLabelText('inside a'))
    expect(a.open).toBe(true)
    fireEvent.pointerDown(screen.getByRole('button', { name: 'Elsewhere' }))
    expect(a.open).toBe(false)
  })

  it('closes on Escape and returns focus to the summary', () => {
    render(<Harness />)
    const a = open('a')
    screen.getByLabelText('inside a').focus()
    fireEvent.keyDown(screen.getByLabelText('inside a'), { key: 'Escape' })
    expect(a.open).toBe(false)
    expect(screen.getByText('First')).toHaveFocus()
  })

  it('closes when focus moves out of the footer', () => {
    render(<Harness />)
    const a = open('a')
    screen.getByRole('button', { name: 'Elsewhere' }).focus()
    expect(a.open).toBe(false)
  })
})
