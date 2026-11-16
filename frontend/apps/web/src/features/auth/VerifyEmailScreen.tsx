import { useEffect, useRef, useState } from 'react'
import { api, errorMessage } from '../../lib/api'

type State = { status: 'working' } | { status: 'done' } | { status: 'failed'; message: string }

/** Landing page for the emailed link. It works without a session, so it can be opened in any browser. */
export function VerifyEmailScreen({ token, onContinue }: { token: string; onContinue: () => void }) {
  const [state, setState] = useState<State>(token ? { status: 'working' } : { status: 'failed', message: 'This link is incomplete. Request a new verification email from your account settings.' })
  const started = useRef(false)

  useEffect(() => {
    // The link is single-use, so send it exactly once even when effects run twice in development.
    if (!token || started.current) return
    started.current = true
    api('/api/v1/auth/email/verify', { method: 'POST', body: JSON.stringify({ token }) })
      .then(() => setState({ status: 'done' }))
      .catch((cause: unknown) => setState({ status: 'failed', message: errorMessage(cause, 'Could not confirm this address.') }))
  }, [token])

  return (
    <main className="auth-screen">
      <section className="auth-panel">
        <div className="auth-form-wrap">
          <p className="eyebrow">EMAIL ADDRESS</p>
          <h2>{state.status === 'done' ? 'Address confirmed' : state.status === 'failed' ? 'Could not confirm' : 'Confirming your address…'}</h2>
          {state.status === 'working' && <p className="muted" role="status">One moment while we confirm your email address.</p>}
          {state.status === 'done' && <div className="notice success" role="status">Your email address is confirmed. It can now be used to recover your account.</div>}
          {state.status === 'failed' && <div className="notice error" role="alert">{state.message}</div>}
          {state.status !== 'working' && <button className="primary-button" onClick={onContinue}>Continue to RustChat</button>}
        </div>
      </section>
    </main>
  )
}
