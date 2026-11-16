import { describe, expect, it } from 'vitest'
import { displayName, mergeBySequence } from './format'
import { uuid7 } from './ids'
import type { Conversation } from '../types'

describe('uuid7', () => {
  it('produces a version 7, variant 1 UUID that sorts by creation time', () => {
    const first = uuid7()
    expect(first).toMatch(/^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/)
    const later = (() => { const end = Date.now() + 5; while (Date.now() < end) { /* wait for the clock to advance */ } return uuid7() })()
    expect(later > first).toBe(true)
  })

  it('does not repeat', () => {
    expect(new Set(Array.from({ length: 200 }, uuid7)).size).toBe(200)
  })
})

describe('mergeBySequence', () => {
  it('replaces by id, adds new items and keeps sequence order', () => {
    const current = [{ id: 'a', sequence: 1, text: 'old' }, { id: 'c', sequence: 3, text: 'c' }]
    const merged = mergeBySequence(current, [{ id: 'b', sequence: 2, text: 'b' }, { id: 'a', sequence: 1, text: 'edited' }])
    expect(merged.map((item) => `${item.sequence}:${item.text}`)).toEqual(['1:edited', '2:b', '3:c'])
  })
})

describe('displayName', () => {
  const direct: Conversation = { id: 'conversation-1', kind: 'direct', created_by: 'me', created_at: '', members: ['me', 'peer-0000-1111'] }

  it('shows the peer username when known and a short id otherwise', () => {
    expect(displayName(direct, 'me', { 'peer-0000-1111': { id: 'peer-0000-1111', username: 'bobby', created_at: '' } })).toBe('bobby')
    expect(displayName(direct, 'me')).toBe('Direct · peer-000')
  })

  it('labels groups with their size', () => {
    expect(displayName({ ...direct, kind: 'group', members: ['me', 'a', 'b'] }, 'me')).toBe('Group conversation · 3')
  })
})
