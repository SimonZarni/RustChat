import { useEffect, useState } from 'react'

export type Theme = 'light' | 'dark'

const STORAGE_KEY = 'rustchat-theme'

function storedTheme(): Theme {
  try { return localStorage.getItem(STORAGE_KEY) === 'dark' ? 'dark' : 'light' } catch { return 'light' }
}

/** Light/dark preference, persisted in the browser and applied to the document root. */
export function useTheme(): [Theme, (theme: Theme) => void] {
  const [theme, setTheme] = useState<Theme>(storedTheme)
  useEffect(() => {
    document.documentElement.dataset.theme = theme
    try { localStorage.setItem(STORAGE_KEY, theme) } catch { /* The preference just will not persist. */ }
  }, [theme])
  return [theme, setTheme]
}
