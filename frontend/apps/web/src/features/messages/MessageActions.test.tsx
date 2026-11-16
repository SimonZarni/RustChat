import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { Message } from '../../types'
import { MessageActions } from './MessageActions'

afterEach(cleanup)

const message = { id: 'm1' } as Message

function setup(own: boolean) {
  const handlers = { onToggleReaction: vi.fn(), onReply: vi.fn(), onEdit: vi.fn(), onDelete: vi.fn() }
  render(<MessageActions message={message} own={own} reactions={[]} pendingReactions={{}} {...handlers} />)
  return handlers
}

describe('MessageActions', () => {
  it('gives other people\'s messages only React and Reply', () => {
    const { onReply } = setup(false)
    expect(screen.getByRole('group', { name: 'Message actions' })).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'More actions' })).not.toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Reply' }))
    expect(onReply).toHaveBeenCalledWith(message)
    expect(screen.getByRole('button', { name: 'React' })).toBeInTheDocument()
  })

  it('opens a menu with Edit and Delete on your own messages and focuses the first item', () => {
    setup(true)
    const more = screen.getByRole('button', { name: 'More actions' })
    expect(more).toHaveAttribute('aria-expanded', 'false')
    fireEvent.click(more)
    expect(more).toHaveAttribute('aria-expanded', 'true')
    expect(screen.getByRole('menu', { name: 'More message actions' })).toBeInTheDocument()
    expect(screen.getByRole('menuitem', { name: 'Edit' })).toHaveFocus()
  })

  it('moves between items with the arrow keys and closes on Escape, returning focus', () => {
    setup(true)
    const more = screen.getByRole('button', { name: 'More actions' })
    fireEvent.click(more)
    const menu = screen.getByRole('menu')
    fireEvent.keyDown(menu, { key: 'ArrowDown' })
    expect(screen.getByRole('menuitem', { name: 'Delete' })).toHaveFocus()
    fireEvent.keyDown(menu, { key: 'ArrowDown' })
    expect(screen.getByRole('menuitem', { name: 'Edit' })).toHaveFocus()
    fireEvent.keyDown(menu, { key: 'End' })
    expect(screen.getByRole('menuitem', { name: 'Delete' })).toHaveFocus()
    fireEvent.keyDown(menu, { key: 'Escape' })
    expect(screen.queryByRole('menu')).not.toBeInTheDocument()
    expect(more).toHaveFocus()
  })

  it('runs Edit or Delete, closes the menu and hands focus to the menu button first', () => {
    const { onEdit, onDelete } = setup(true)
    const more = screen.getByRole('button', { name: 'More actions' })
    fireEvent.click(more)
    fireEvent.click(screen.getByRole('menuitem', { name: 'Edit' }))
    expect(onEdit).toHaveBeenCalledWith(message)
    expect(screen.queryByRole('menu')).not.toBeInTheDocument()
    expect(more).toHaveFocus()
    fireEvent.click(more)
    fireEvent.click(screen.getByRole('menuitem', { name: 'Delete' }))
    expect(onDelete).toHaveBeenCalledWith(message)
  })

  it('closes the menu on an outside pointer press', () => {
    setup(true)
    fireEvent.click(screen.getByRole('button', { name: 'More actions' }))
    fireEvent.pointerDown(document.body)
    expect(screen.queryByRole('menu')).not.toBeInTheDocument()
  })
})
