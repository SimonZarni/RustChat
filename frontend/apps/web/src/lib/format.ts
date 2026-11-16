import type { Conversation, User } from '../types'
import { shortId } from './ids'

export const conversationShortcut = typeof navigator !== 'undefined' && /Mac|iPhone|iPad/.test(navigator.platform) ? '⌘ K' : 'Ctrl K'

export function displayName(conversation: Conversation, currentUserId: string, profiles: Record<string, User> = {}): string {
  if (conversation.kind === 'group') return conversation.name || `Group conversation · ${conversation.members.length}`
  const peer = conversation.members.find((member) => member !== currentUserId)
  return peer ? profiles[peer]?.username ?? `Direct · ${shortId(peer)}` : 'Direct conversation'
}

export function timeLabel(value: string): string {
  return new Intl.DateTimeFormat(undefined, { hour: 'numeric', minute: '2-digit' }).format(new Date(value))
}

/** Merges messages by id and keeps them in sequence order. */
export function mergeBySequence<T extends { id: string; sequence: number }>(current: T[], incoming: T[]): T[] {
  const byId = new Map(current.map((item) => [item.id, item]))
  for (const item of incoming) byId.set(item.id, item)
  return [...byId.values()].sort((a, b) => a.sequence - b.sequence)
}
