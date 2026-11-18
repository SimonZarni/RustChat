export type User = { id: string; username: string; created_at: string }
export type UserProfile = { id: string; username: string; display_name: string; bio: string; updated_at: string }
export type Session = { id: string; created_at: string; expires_at: string; current: boolean }
export type ProfilePrivacy = { discoverable: boolean; profile_visibility: 'everyone' | 'conversation_members'; presence_visible: boolean }
export type Conversation = { id: string; kind: 'direct' | 'group'; name?: string | null; created_by: string; created_at: string; members: string[] }
export type Message = {
  id: string
  conversation_id: string
  sender_id: string
  sequence: number
  client_message_id: string
  reply_to_message_id?: string | null
  mentioned_user_ids?: string[]
  content: string
  created_at: string
  edited_at?: string | null
  deleted_at?: string | null
  attachments?: MessageAttachment[]
}
export type MessageReaction = { message_id: string; emoji: string; count: number; reacted_by_me: boolean }
export type Community = { id: string; owner_id: string; name: string; description: string; created_at: string; member_count: number }
export type Channel = {
  id: string
  community_id: string
  conversation_id: string
  category_id: string | null
  channel_type?: 'text' | 'voice'
  name: string
  description: string
  position: number
  read_only: boolean
  created_at: string
}
export type Category = { id: string; community_id: string; name: string; position: number }
export type CommunityRole = { id: string; community_id: string; name: string; position: number; permissions: number; is_default: boolean }
export type CommunityInvite = {
  id: string
  created_at: string
  expires_at: string | null
  max_uses: number | null
  use_count: number
  revoked_at: string | null
}
export type InstanceUser = {
  id: string
  username: string
  created_at: string
  is_instance_admin: boolean
  deactivated_at: string | null
  suspended_at: string | null
  deleted_at: string | null
}

export type MessageAttachment = {
  id: string
  message_id: string | null
  file_name: string
  media_type: string
  byte_size: number
  created_at: string
}
export type ReadCursor = { user_id: string; sequence: number }
export type EmailStatus = { email: string | null; verified: boolean }
export type MfaStatus = { available: boolean; enabled: boolean; recovery_codes_remaining: number }
export type LoginResponse = { user: User } | { mfa_required: true; mfa_token: string }
export type AuditUser = { id: string; username: string }
export type AuditEvent = {
  id: string
  event_type: string
  actor: AuditUser | null
  target: AuditUser | null
  details: Record<string, unknown>
  created_at: string
}
export type AuditPage = { events: AuditEvent[]; next_cursor: string | null }
