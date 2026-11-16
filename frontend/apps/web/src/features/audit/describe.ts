import type { AuditEvent } from '../../types'

const LABELS: Record<string, (event: AuditEvent) => string> = {
  'user.suspended': (event) => `${event.actor?.username ?? 'An administrator'} suspended ${event.target?.username ?? 'an account'}`,
  'user.unsuspended': (event) => `${event.actor?.username ?? 'An administrator'} restored ${event.target?.username ?? 'an account'}`,
  'settings.updated': (event) => `${event.actor?.username ?? 'Someone'} updated the community settings`,
  'ownership.transferred': (event) => `${event.actor?.username ?? 'Someone'} transferred ownership`,
  'community.deleted': (event) => `${event.actor?.username ?? 'Someone'} deleted the community`,
  'member.removed': (event) => `${event.actor?.username ?? 'Someone'} removed a member`,
  'invite.created': (event) => `${event.actor?.username ?? 'Someone'} created an invitation`,
  'invite.revoked': (event) => `${event.actor?.username ?? 'Someone'} revoked an invitation`,
}

/** A readable sentence for an audit event; unknown event types show their raw name. */
export function describeAuditEvent(event: AuditEvent): string {
  return LABELS[event.event_type]?.(event) ?? event.event_type
}
