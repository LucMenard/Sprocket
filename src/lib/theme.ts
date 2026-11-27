// Adapted from Tracket (MIT, Copyright (c) 2026 Hydhen): same three-way choice, kept in localStorage.
import { getCurrentWindow } from '@tauri-apps/api/window'
import { useSyncExternalStore } from 'react'

export type ThemePreference = 'system' | 'light' | 'dark'

const STORAGE_KEY = 'sprocket-theme'
const systemDark = window.matchMedia('(prefers-color-scheme: dark)')
const inTauri = '__TAURI_INTERNALS__' in window

export function getThemePreference(): ThemePreference {
  try {
    const stored = localStorage.getItem(STORAGE_KEY)
    if (stored === 'light' || stored === 'dark') return stored
  } catch {
    // storage unavailable: follow the system
  }
  return 'system'
}

function apply() {
  const preference = getThemePreference()
  const dark = preference === 'dark' || (preference === 'system' && systemDark.matches)
  document.documentElement.classList.toggle('dark', dark)
  if (inTauri) {
    // Cosmetic only: the native parts of the window follow the choice too
    getCurrentWindow()
      .setTheme(preference === 'system' ? null : preference)
      .catch(() => {})
  }
}

export function setThemePreference(preference: ThemePreference) {
  try {
    if (preference === 'system') localStorage.removeItem(STORAGE_KEY)
    else localStorage.setItem(STORAGE_KEY, preference)
  } catch {
    // still applies for this session
  }
  apply()
  listeners.forEach((listener) => listener())
}

const listeners = new Set<() => void>()

/** The choice, kept current wherever it is changed (the header's button or Settings). */
export function useThemePreference(): ThemePreference {
  return useSyncExternalStore((listener) => {
    listeners.add(listener)
    return () => listeners.delete(listener)
  }, getThemePreference)
}

export function initTheme() {
  apply()
  systemDark.addEventListener('change', apply)
}
