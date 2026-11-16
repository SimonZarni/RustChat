import { useCallback, useMemo, useState, type FormEvent, type ReactNode } from 'react'
import { Dialog } from './Dialog'
import { DialogsContext, type ConfirmOptions, type PromptOptions } from './dialogs-context'

type Pending =
  | { kind: 'confirm'; options: ConfirmOptions; resolve: (value: boolean) => void }
  | { kind: 'prompt'; options: PromptOptions; resolve: (value: string | null) => void }

/**
 * Accessible replacements for `window.confirm` and `window.prompt`: they use the
 * app's focus-trapping dialog, match its styling and work with screen readers.
 */
export function DialogProvider({ children }: { children: ReactNode }) {
  const [pending, setPending] = useState<Pending | null>(null)

  const confirm = useCallback((options: ConfirmOptions) => new Promise<boolean>((resolve) => setPending({ kind: 'confirm', options, resolve })), [])
  const prompt = useCallback((options: PromptOptions) => new Promise<string | null>((resolve) => setPending({ kind: 'prompt', options, resolve })), [])
  const value = useMemo(() => ({ confirm, prompt }), [confirm, prompt])

  function finish(result: boolean | string | null) {
    if (!pending) return
    setPending(null)
    if (pending.kind === 'confirm') pending.resolve(result === true)
    else pending.resolve(typeof result === 'string' ? result : null)
  }

  return (
    <DialogsContext.Provider value={value}>
      {children}
      {pending?.kind === 'confirm' && <ConfirmDialog options={pending.options} onDone={(result) => finish(result)} />}
      {pending?.kind === 'prompt' && <PromptDialog options={pending.options} onDone={(result) => finish(result)} />}
    </DialogsContext.Provider>
  )
}

function ConfirmDialog({ options, onDone }: { options: ConfirmOptions; onDone: (result: boolean) => void }) {
  return (
    <Dialog titleId="confirm-title" onClose={() => onDone(false)}>
      <h2 id="confirm-title">{options.title}</h2>
      {options.detail && <p className="muted">{options.detail}</p>}
      <div className="dialog-actions">
        <button type="button" className="outline-button" data-autofocus={options.danger ? '' : undefined} onClick={() => onDone(false)}>Cancel</button>
        <button type="button" className={options.danger ? 'danger-button' : 'primary-button'} data-autofocus={options.danger ? undefined : ''} onClick={() => onDone(true)}>{options.confirmLabel ?? 'Confirm'}</button>
      </div>
    </Dialog>
  )
}

function PromptDialog({ options, onDone }: { options: PromptOptions; onDone: (result: string | null) => void }) {
  const [text, setText] = useState(options.initialValue ?? '')
  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    onDone(text)
  }
  return (
    <Dialog titleId="prompt-title" onClose={() => onDone(null)}>
      <h2 id="prompt-title">{options.title}</h2>
      <form className="stack-form" onSubmit={submit}>
        <label>{options.label}
          {options.multiline
            ? <textarea autoFocus rows={4} maxLength={options.maxLength} value={text} onChange={(event) => setText(event.target.value)} />
            : <input autoFocus maxLength={options.maxLength} value={text} onChange={(event) => setText(event.target.value)} />}
        </label>
        <div className="dialog-actions">
          <button type="button" className="outline-button" onClick={() => onDone(null)}>Cancel</button>
          <button className="primary-button">{options.confirmLabel ?? 'Save'}</button>
        </div>
      </form>
    </Dialog>
  )
}
