import { useState, type FormEvent } from 'react'
import { api, errorMessage } from '../../lib/api'
import type { LoginResponse, User } from '../../types'

type Props = {
  onAuthenticated: (user: User) => void
  /** Message to show on arrival, for example after the account was closed. */
  initialNotice?: string
}

/** Password-reset links look like `/reset-password?token=...`. */
function readRecoveryToken(): string {
  const path = window.location.pathname
  return path.endsWith('/reset-password') ? new URLSearchParams(window.location.search).get('token') ?? '' : ''
}

export function AuthScreen({ onAuthenticated, initialNotice = '' }: Props) {
  const [authMode, setAuthMode] = useState<'login' | 'register'>('login')
  const [recoveryToken] = useState(readRecoveryToken)
  const [recoveryMode, setRecoveryMode] = useState<'request' | 'reset' | null>(() => recoveryToken ? 'reset' : null)
  const [recoveryPassword, setRecoveryPassword] = useState('')
  const [email, setEmail] = useState('')
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  /** Set when the password was accepted and a code from the second factor is still needed. */
  const [mfaToken, setMfaToken] = useState<string | null>(null)
  const [mfaCode, setMfaCode] = useState('')
  const [notice, setNotice] = useState(initialNotice)
  const [error, setError] = useState('')
  const [busy, setBusy] = useState(false)

  async function submitAuth(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); setBusy(true); setNotice(''); setError('')
    try {
      if (authMode === 'register') {
        await api('/api/v1/auth/register', { method: 'POST', body: JSON.stringify({ username, password, email }) })
        setAuthMode('login'); setNotice('Account created. Sign in with your new credentials.')
      } else {
        const result = await api<LoginResponse>('/api/v1/auth/login', { method: 'POST', body: JSON.stringify({ username, password }) })
        setPassword('')
        if ('mfa_required' in result) setMfaToken(result.mfa_token)
        else onAuthenticated(result.user)
      }
    } catch (cause) { setError(errorMessage(cause, 'Sign in failed')) }
    finally { setBusy(false) }
  }

  async function submitMfa(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); setBusy(true); setError('')
    try {
      const result = await api<{ user: User }>('/api/v1/auth/login/mfa', { method: 'POST', body: JSON.stringify({ mfa_token: mfaToken, code: mfaCode }) })
      onAuthenticated(result.user)
    } catch (cause) {
      setError(errorMessage(cause, 'That code was not accepted'))
      setMfaCode('')
    } finally { setBusy(false) }
  }

  async function submitPasswordRecovery(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); setBusy(true); setError(''); setNotice('')
    try {
      if (recoveryMode === 'reset') {
        await api('/api/v1/auth/password-reset/complete', { method: 'POST', body: JSON.stringify({ token: recoveryToken, new_password: recoveryPassword }) })
        window.history.replaceState({}, '', '/')
        setRecoveryMode(null); setRecoveryPassword(''); setNotice('Password updated. Sign in with your new password.')
      } else {
        await api('/api/v1/auth/password-reset/request', { method: 'POST', body: JSON.stringify({ email }) })
        setNotice('If an eligible account matches that address, a reset link will arrive by email.')
      }
    } catch (cause) { setError(errorMessage(cause, 'Could not complete password recovery')) }
    finally { setBusy(false) }
  }

  const heading = mfaToken ? 'Verification code' : recoveryMode === 'reset' ? 'Choose a new password'
    : recoveryMode === 'request' ? 'Reset your password'
      : authMode === 'login' ? 'Welcome back' : 'Create your account'
  const subheading = mfaToken ? 'Enter the code from your authenticator app, or one of your recovery codes.' : recoveryMode ? 'Use the secure link sent to your email address.'
    : authMode === 'login' ? 'Sign in to pick up where you left off.' : 'Choose a username, email address, and a strong password.'

  return (
    <main className="auth-screen">
      <section className="auth-intro">
        <div className="brand-lockup"><span className="brand-mark">R</span><span>RustChat</span></div>
        <p className="eyebrow">A quieter place to talk</p>
        <h1>Good conversations<br />start here.</h1>
        <p className="intro-copy">A self-hosted space for the people and communities you care about.</p>
        <div className="intro-rule"><span />PRIVATE BY DESIGN</div>
      </section>
      <section className="auth-panel">
        <div className="auth-form-wrap">
          <p className="eyebrow">YOUR SPACE</p>
          <h2>{heading}</h2>
          <p className="muted">{subheading}</p>
          {error && <div className="notice error" role="alert">{error}</div>}
          {notice && <div className="notice success" role="status">{notice}</div>}
          {mfaToken ? (
            <form className="stack-form" onSubmit={submitMfa}>
              <label>Code<input autoComplete="one-time-code" inputMode="text" autoFocus required maxLength={32} value={mfaCode} onChange={(event) => setMfaCode(event.target.value)} /></label>
              <button className="primary-button" disabled={busy}>{busy ? 'Please wait…' : 'Verify and sign in'}</button>
              <button type="button" className="text-button" onClick={() => { setMfaToken(null); setMfaCode(''); setError('') }}>Back to sign in</button>
            </form>
          ) : recoveryMode ? (
            <form className="stack-form" onSubmit={submitPasswordRecovery}>
              {recoveryMode === 'request'
                ? <label>Email<input autoComplete="email" type="email" required value={email} onChange={(event) => setEmail(event.target.value)} /></label>
                : <label>New password<input type="password" autoComplete="new-password" minLength={12} maxLength={128} required value={recoveryPassword} onChange={(event) => setRecoveryPassword(event.target.value)} /></label>}
              <button className="primary-button" disabled={busy}>{busy ? 'Please wait…' : recoveryMode === 'request' ? 'Send reset link' : 'Update password'}</button>
              <button type="button" className="text-button" onClick={() => { setRecoveryMode(null); setError('') }}>Back to sign in</button>
            </form>
          ) : (
            <>
              <form className="stack-form" onSubmit={submitAuth}>
                <label>Username<input autoComplete="username" minLength={3} maxLength={32} required value={username} onChange={(event) => setUsername(event.target.value)} /></label>
                {authMode === 'register' && <label>Email<input autoComplete="email" type="email" required value={email} onChange={(event) => setEmail(event.target.value)} /></label>}
                <label>Password<input type="password" autoComplete={authMode === 'login' ? 'current-password' : 'new-password'} minLength={12} maxLength={128} required value={password} onChange={(event) => setPassword(event.target.value)} /></label>
                <button className="primary-button" disabled={busy}>{busy ? 'Please wait…' : authMode === 'login' ? 'Sign in' : 'Create account'}</button>
              </form>
              {authMode === 'login' && <button className="text-button" onClick={() => { setRecoveryMode('request'); setNotice(''); setError('') }}>Forgot password?</button>}
              <p className="switch-auth">
                {authMode === 'login' ? 'New to this space?' : 'Already have an account?'}{' '}
                <button className="text-button" onClick={() => { setAuthMode(authMode === 'login' ? 'register' : 'login'); setError(''); setNotice('') }}>{authMode === 'login' ? 'Create an account' : 'Sign in'}</button>
              </p>
            </>
          )}
        </div>
        <div className="auth-foot"><span>RUSTCHAT · SELF-HOSTED MESSAGING</span><span>01 / 01</span></div>
      </section>
    </main>
  )
}
