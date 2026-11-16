/** Verification links look like `/verify-email?token=...`; returns the token, or null on any other page. */
export function readVerifyEmailToken(): string | null {
  if (!window.location.pathname.endsWith('/verify-email')) return null
  return new URLSearchParams(window.location.search).get('token') ?? ''
}
