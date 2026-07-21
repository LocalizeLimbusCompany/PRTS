import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'

const apiMock = vi.hoisted(() => ({
  authApi: {
    login: vi.fn(),
    logout: vi.fn(),
    register: vi.fn(),
  },
  usersApi: { me: vi.fn() },
}))

vi.mock('@/api', () => apiMock)
vi.mock('@/stores/aiExplanationSession', () => ({
  useAiExplanationSessionStore: () => ({ clearAll: vi.fn() }),
}))

import { setTokens } from '@/api/session'
import { useAuthStore } from './auth'

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

describe('auth session restoration', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    vi.stubGlobal('localStorage', new MemoryStorage())
    setActivePinia(createPinia())
    apiMock.usersApi.me.mockReset()
  })

  afterEach(() => {
    vi.useRealTimers()
    vi.unstubAllGlobals()
  })

  it('keeps a transiently unavailable session eligible for routes and retries restoration', async () => {
    setTokens('access', 'refresh')
    apiMock.usersApi.me.mockRejectedValueOnce(new Error('offline')).mockResolvedValueOnce({ id: 7 })
    const auth = useAuthStore()

    await auth.ensureReady()
    expect(auth.isAuthed).toBe(false)
    expect(auth.hasSession).toBe(true)

    await vi.advanceTimersByTimeAsync(1_000)
    expect(apiMock.usersApi.me).toHaveBeenCalledTimes(2)
    expect(auth.isAuthed).toBe(true)
  })
})
