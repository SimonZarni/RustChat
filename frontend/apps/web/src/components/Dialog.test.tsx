import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { useState } from 'react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { Dialog } from './Dialog'

afterEach(cleanup)

function Harness({ onClose }: { onClose?: () => void }) {
  const [open, setOpen] = useState(false)
  return (
    <>
      <button onClick={() => setOpen(true)}>Open</button>
      {open && (
        <Dialog titleId="t" onClose={() => { setOpen(false); onClose?.() }} closeOnBackdrop>
          <h2 id="t">Title</h2>
          <input aria-label="First" autoFocus />
          <button>Last</button>
        </Dialog>
      )}
    </>
  )
}

describe('Dialog', () => {
  it('focuses its first field, traps Tab and returns focus to the opener on Escape', () => {
    const onClose = vi.fn()
    render(<Harness onClose={onClose} />)
    const opener = screen.getByRole('button', { name: 'Open' })
    opener.focus()
    fireEvent.click(opener)

    expect(screen.getByRole('dialog', { name: 'Title' })).toBeInTheDocument()
    expect(screen.getByLabelText('First')).toHaveFocus()

    screen.getByRole('button', { name: 'Last' }).focus()
    fireEvent.keyDown(document.activeElement!, { key: 'Tab' })
    expect(screen.getByRole('button', { name: 'Close' })).toHaveFocus()
    fireEvent.keyDown(document.activeElement!, { key: 'Tab', shiftKey: true })
    expect(screen.getByRole('button', { name: 'Last' })).toHaveFocus()

    fireEvent.keyDown(window, { key: 'Escape' })
    expect(onClose).toHaveBeenCalledTimes(1)
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
    expect(opener).toHaveFocus()
  })

  it('closes when the backdrop is pressed but not when the dialog itself is', () => {
    render(<Harness />)
    fireEvent.click(screen.getByRole('button', { name: 'Open' }))
    fireEvent.mouseDown(screen.getByRole('dialog'))
    expect(screen.getByRole('dialog')).toBeInTheDocument()
    fireEvent.mouseDown(screen.getByRole('dialog').parentElement!)
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
  })
})
