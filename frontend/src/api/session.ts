// 令牌存储（localStorage）。被 axios 拦截器与 auth store 共用，避免循环依赖。

const SESSION = 'prts_session_v1'
const LEGACY_ACCESS = 'prts_access'
const LEGACY_REFRESH = 'prts_refresh'

interface StoredSession {
  accessToken: string
  refreshToken: string
}

type SessionListener = () => void

const listeners = new Set<SessionListener>()

/** Parse one atomically stored token pair and reject partial or malformed records. */
function parseSession(raw: string | null): StoredSession | null {
  if (!raw) return null
  try {
    const value = JSON.parse(raw) as Partial<StoredSession>
    if (typeof value.accessToken !== 'string' || typeof value.refreshToken !== 'string') return null
    if (!value.accessToken || !value.refreshToken) return null
    return { accessToken: value.accessToken, refreshToken: value.refreshToken }
  } catch {
    return null
  }
}

/** Read the current token pair and migrate the pre-v1 split keys when both are available. */
function readSession(): StoredSession | null {
  if (typeof localStorage === 'undefined') return null
  const current = parseSession(localStorage.getItem(SESSION))
  if (current) return current

  const accessToken = localStorage.getItem(LEGACY_ACCESS)
  const refreshToken = localStorage.getItem(LEGACY_REFRESH)
  if (!accessToken || !refreshToken) return null
  const migrated = { accessToken, refreshToken }
  localStorage.setItem(SESSION, JSON.stringify(migrated))
  localStorage.removeItem(LEGACY_ACCESS)
  localStorage.removeItem(LEGACY_REFRESH)
  return migrated
}

function notifySessionChange(): void {
  for (const listener of listeners) listener()
}

export function getAccessToken(): string | null {
  return readSession()?.accessToken ?? null
}

export function getRefreshToken(): string | null {
  return readSession()?.refreshToken ?? null
}

export function setTokens(access: string, refresh: string): void {
  localStorage.setItem(
    SESSION,
    JSON.stringify({ accessToken: access, refreshToken: refresh } satisfies StoredSession),
  )
  localStorage.removeItem(LEGACY_ACCESS)
  localStorage.removeItem(LEGACY_REFRESH)
  notifySessionChange()
}

export function clearTokens(): void {
  localStorage.removeItem(SESSION)
  localStorage.removeItem(LEGACY_ACCESS)
  localStorage.removeItem(LEGACY_REFRESH)
  notifySessionChange()
}

/** Observe same-tab mutations and cross-tab storage changes without coupling to Pinia. */
export function onSessionChange(listener: SessionListener): () => void {
  listeners.add(listener)
  return () => listeners.delete(listener)
}

if (typeof window !== 'undefined') {
  window.addEventListener('storage', (event) => {
    if ([SESSION, LEGACY_ACCESS, LEGACY_REFRESH].includes(event.key ?? '')) notifySessionChange()
  })
}
