import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { Message, MessageReaction } from '../../types'
import { ReactButton, ReactionChips } from './ReactionControls'

afterEach(cleanup)

const message = { id: 'm1' } as Message
const reaction = (emoji: string, count: number, mine = false) => ({ message_id: 'm1', emoji, count, reacted_by_me: mine }) as MessageReaction

describe('reaction controls', () => {
  it('draws nothing when no one has reacted', () => {
    const { container } = render(<ReactionChips message={message} reactions={[]} pending={{}} onToggle={() => {}} />)
    expect(container).toBeEmptyDOMElement()
  })

  it('shows only the reactions that exist, with counts and my own marked', () => {
    const onToggle = vi.fn()
    render(<ReactionChips message={message} reactions={[reaction('👍', 2, true), reaction('🎉', 1)]} pending={{}} onToggle={onToggle} />)
    expect(screen.getAllByRole('button')).toHaveLength(2)
    expect(screen.getByRole('button', { name: 'Remove 👍 reaction, 2 total' })).toHaveAttribute('aria-pressed', 'true')
    fireEvent.click(screen.getByRole('button', { name: 'Add 🎉 reaction, 1 total' }))
    expect(onToggle).toHaveBeenCalledWith(message, '🎉')
  })

  it('opens a picker from React, reacts, closes and returns focus', () => {
    const onToggle = vi.fn()
    render(<ReactButton message={message} reactions={[]} pending={{}} onToggle={onToggle} />)
    expect(screen.queryByRole('group', { name: 'Choose a reaction' })).not.toBeInTheDocument()
    const opener = screen.getByRole('button', { name: 'React' })
    fireEvent.click(opener)
    expect(opener).toHaveAttribute('aria-expanded', 'true')
    expect(screen.getAllByRole('button').filter((item) => item !== opener)).toHaveLength(6)
    fireEvent.click(screen.getByRole('button', { name: 'Add ❤️ reaction' }))
    expect(onToggle).toHaveBeenCalledWith(message, '❤️')
    expect(screen.queryByRole('group', { name: 'Choose a reaction' })).not.toBeInTheDocument()
    expect(opener).toHaveFocus()
  })

  it('closes the picker on Escape and on an outside click', () => {
    render(<><ReactButton message={message} reactions={[]} pending={{}} onToggle={() => {}} /><button>Elsewhere</button></>)
    fireEvent.click(screen.getByRole('button', { name: 'React' }))
    fireEvent.keyDown(document, { key: 'Escape' })
    expect(screen.queryByRole('group', { name: 'Choose a reaction' })).not.toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'React' }))
    fireEvent.pointerDown(screen.getByRole('button', { name: 'Elsewhere' }))
    expect(screen.queryByRole('group', { name: 'Choose a reaction' })).not.toBeInTheDocument()
  })
})
