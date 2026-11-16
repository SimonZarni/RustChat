/** Client-generated UUIDv7, used as the idempotency key for message sends. */
export function uuid7(): string {
  const bytes = crypto.getRandomValues(new Uint8Array(16))
  let millis = BigInt(Date.now())
  for (let i = 5; i >= 0; i--) { bytes[i] = Number(millis & 255n); millis >>= 8n }
  bytes[6] = ((bytes[6] ?? 0) & 0x0f) | 0x70
  bytes[8] = ((bytes[8] ?? 0) & 0x3f) | 0x80
  const hex = [...bytes].map((byte) => byte.toString(16).padStart(2, '0')).join('')
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`
}

export function shortId(id: string): string { return id.slice(0, 8) }

export function initial(value: string): string { return value.slice(0, 1).toUpperCase() }
