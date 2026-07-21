import { beforeEach, describe, expect, it, vi } from 'vitest'

const axiosMock = vi.hoisted(() => {
  let responseReject: ((error: unknown) => Promise<unknown>) | null = null
  const post = vi.fn()
  const instance = {
    interceptors: {
      request: { use: vi.fn() },
      response: {
        use: vi.fn((_success, reject: (error: unknown) => Promise<unknown>) => {
          responseReject = reject
        }),
      },
    },
  }
  return {
    post,
    instance,
    reject(error: unknown) {
      if (!responseReject) throw new Error('response interceptor was not registered')
      return responseReject(error)
    },
  }
})

vi.mock('axios', () => ({
  default: {
    create: vi.fn(() => axiosMock.instance),
    post: axiosMock.post,
  },
}))

vi.mock('@/i18n', () => ({
  i18n: { global: { locale: { value: 'en' }, t: (key: string) => key } },
}))

import './http'
import { getAccessToken, getRefreshToken, setTokens } from './session'

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

function expiredRequestError() {
  return {
    response: { status: 401 },
    config: { headers: { Authorization: 'Bearer expired-access' } },
  }
}

describe('HTTP session refresh reliability', () => {
  beforeEach(() => {
    vi.stubGlobal('localStorage', new MemoryStorage())
    vi.stubGlobal('location', { hash: '#/projects/1' })
    vi.stubGlobal('navigator', {})
    axiosMock.post.mockReset()
    setTokens('expired-access', 'valid-refresh')
  })

  it('preserves tokens when refresh fails because the service is temporarily unavailable', async () => {
    const serviceUnavailable = { response: { status: 503 }, message: 'temporarily unavailable' }
    axiosMock.post.mockRejectedValueOnce(serviceUnavailable)

    await expect(axiosMock.reject(expiredRequestError())).rejects.toBe(serviceUnavailable)
    expect(getAccessToken()).toBe('expired-access')
    expect(getRefreshToken()).toBe('valid-refresh')
    expect(location.hash).toBe('#/projects/1')
  })

  it('clears tokens only after refresh is authoritatively rejected', async () => {
    axiosMock.post.mockRejectedValueOnce({ response: { status: 401 } })
    const original = expiredRequestError()

    await expect(axiosMock.reject(original)).rejects.toBe(original)
    expect(getAccessToken()).toBeNull()
    expect(getRefreshToken()).toBeNull()
    expect(location.hash).toBe('#/login')
  })
})
