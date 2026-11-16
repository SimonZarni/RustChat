type ApiError = { error?: { message?: string; code?: string } }

/** Calls the RustChat API with the session cookie; throws an Error carrying the server's message. */
export async function api<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(path, {
    ...init,
    credentials: 'include',
    headers: { ...(init?.body ? { 'Content-Type': 'application/json' } : {}), ...init?.headers },
  })
  if (response.status === 204) return undefined as T
  const data = await response.json().catch(() => ({})) as T & ApiError
  if (!response.ok) throw new Error(data.error?.message ?? data.error?.code ?? `Request failed (${response.status})`)
  return data
}

export function errorMessage(cause: unknown, fallback: string): string {
  return cause instanceof Error ? cause.message : fallback
}
