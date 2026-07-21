import { beforeEach, describe, expect, it, vi } from 'vitest'

class MemoryStorage implements Storage {
  private readonly values = new Map<string, string>()

  get length() {
    return this.values.size
  }

  clear(): void {
    this.values.clear()
  }

  getItem(key: string): string | null {
    return this.values.get(key) ?? null
  }

  key(index: number): string | null {
    return [...this.values.keys()][index] ?? null
  }

  removeItem(key: string): void {
    this.values.delete(key)
  }

  setItem(key: string, value: string): void {
    this.values.set(key, value)
  }
}

describe('session token storage', () => {
  beforeEach(() => {
    vi.resetModules()
    vi.stubGlobal('localStorage', new MemoryStorage())
  })

  it('stores each rotated access/refresh pair atomically', async () => {
    const session = await import('./session')
    session.setTokens('access-1', 'refresh-1')
    session.setTokens('access-2', 'refresh-2')

    expect(session.getAccessToken()).toBe('access-2')
    expect(session.getRefreshToken()).toBe('refresh-2')
    expect(localStorage.length).toBe(1)
    expect(localStorage.getItem('prts_session_v1')).toContain('refresh-2')
  })

  it('migrates a complete legacy pair and ignores partial legacy state', async () => {
    localStorage.setItem('prts_access', 'legacy-access')
    localStorage.setItem('prts_refresh', 'legacy-refresh')
    const session = await import('./session')

    expect(session.getAccessToken()).toBe('legacy-access')
    expect(session.getRefreshToken()).toBe('legacy-refresh')
    expect(localStorage.getItem('prts_access')).toBeNull()

    session.clearTokens()
    localStorage.setItem('prts_refresh', 'orphaned-refresh')
    expect(session.getAccessToken()).toBeNull()
    expect(session.getRefreshToken()).toBeNull()
  })

  it('notifies same-tab consumers when the session is cleared', async () => {
    const session = await import('./session')
    const listener = vi.fn()
    const unsubscribe = session.onSessionChange(listener)
    session.setTokens('access', 'refresh')
    session.clearTokens()
    unsubscribe()

    expect(listener).toHaveBeenCalledTimes(2)
  })
})
