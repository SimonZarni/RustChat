/** Community permission bits; must match the backend permission set. */
export const ROLE_PERMISSIONS = [
  ['View community', 1], ['Manage community', 2], ['Manage channels', 4], ['Manage roles', 8],
  ['Manage members', 16], ['Create invites', 32], ['Send messages', 64], ['Read messages', 128], ['Manage messages', 256], ['Connect to voice', 512],
] as const

export const REACTION_EMOJI = ['👍', '❤️', '😂', '🎉', '😮', '😢'] as const
