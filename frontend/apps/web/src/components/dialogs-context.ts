import { createContext, useContext } from 'react'

export type ConfirmOptions = {
  /** The question, for example "Delete this message?". */
  title: string
  /** Optional detail shown under the question. */
  detail?: string
  confirmLabel?: string
  /** Style the confirm button as destructive and focus Cancel first. */
  danger?: boolean
}

export type PromptOptions = {
  title: string
  label: string
  initialValue?: string
  confirmLabel?: string
  /** Use a multi-line field (for message text). */
  multiline?: boolean
  maxLength?: number
}

export type Dialogs = {
  confirm: (options: ConfirmOptions) => Promise<boolean>
  /** Resolves with the entered text, or null when cancelled. */
  prompt: (options: PromptOptions) => Promise<string | null>
}

/** Without a provider (isolated tests) fall back to the browser's own dialogs. */
const nativeFallback: Dialogs = {
  confirm: async ({ title, detail }) => window.confirm(detail ? `${title}\n${detail}` : title),
  prompt: async ({ label, initialValue }) => window.prompt(label, initialValue ?? ''),
}

export const DialogsContext = createContext<Dialogs>(nativeFallback)

export function useDialogs(): Dialogs {
  return useContext(DialogsContext)
}
