import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, describe, expect, it } from 'vitest'
import { DialogProvider } from './Dialogs'
import { useDialogs } from './dialogs-context'

afterEach(cleanup)

function Harness({ onResult }: { onResult: (value: unknown) => void }) {
  const dialogs = useDialogs()
  return (
    <>
      <button onClick={() => void dialogs.confirm({ title: 'Delete this message?', detail: 'Cannot be undone.', confirmLabel: 'Delete', danger: true }).then(onResult)}>Ask</button>
      <button onClick={() => void dialogs.prompt({ title: 'Edit message', label: 'Message', initialValue: 'old text', confirmLabel: 'Save' }).then(onResult)}>Edit</button>
    </>
  )
}

const setup = () => {
  const results: unknown[] = []
  render(<DialogProvider><Harness onResult={(value) => results.push(value)} /></DialogProvider>)
  return results
}

describe('DialogProvider', () => {
  it('confirms with a named dialog that starts on the safe button for destructive actions', async () => {
    const results = setup()
    fireEvent.click(screen.getByRole('button', { name: 'Ask' }))
    expect(await screen.findByRole('dialog', { name: 'Delete this message?' })).toBeInTheDocument()
    expect(screen.getByText('Cannot be undone.')).toBeInTheDocument()
    await waitFor(() => expect(screen.getByRole('button', { name: 'Cancel' })).toHaveFocus())
    fireEvent.click(screen.getByRole('button', { name: 'Delete' }))
    await waitFor(() => expect(results).toEqual([true]))
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
  })

  it('resolves false when cancelled or dismissed with Escape', async () => {
    const results = setup()
    fireEvent.click(screen.getByRole('button', { name: 'Ask' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Cancel' }))
    await waitFor(() => expect(results).toEqual([false]))
    fireEvent.click(screen.getByRole('button', { name: 'Ask' }))
    fireEvent.keyDown(await screen.findByRole('dialog'), { key: 'Escape' })
    await waitFor(() => expect(results).toEqual([false, false]))
  })

  it('prompts with the current text, returns the edit and returns null on cancel', async () => {
    const results = setup()
    fireEvent.click(screen.getByRole('button', { name: 'Edit' }))
    const field = await screen.findByLabelText('Message')
    expect(field).toHaveValue('old text')
    fireEvent.change(field, { target: { value: 'new text' } })
    fireEvent.click(screen.getByRole('button', { name: 'Save' }))
    await waitFor(() => expect(results).toEqual(['new text']))
    fireEvent.click(screen.getByRole('button', { name: 'Edit' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Cancel' }))
    await waitFor(() => expect(results).toEqual(['new text', null]))
  })
})
