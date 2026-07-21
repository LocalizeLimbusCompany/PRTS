import axios, { type AxiosError, type InternalAxiosRequestConfig } from 'axios'

import { clearTokens, getAccessToken, getRefreshToken, setTokens } from './session'
import { i18n } from '@/i18n'

/**
 * 全局 axios 实例。baseURL `/api`，开发由 Vite 代理、生产由 nginx 反代（均剥离 /api 前缀）。
 * 请求自动附带 access token；遇 401 自动用 refresh 轮换一次并重试。
 */
export const http = axios.create({ baseURL: '/api', timeout: 15_000 })

http.interceptors.request.use((config) => {
  config.headers['Accept-Language'] = i18n.global.locale.value
  const token = getAccessToken()
  if (token) {
    config.headers.Authorization = `Bearer ${token}`
  }
  return config
})

// 单飞刷新：并发 401 共享同一次刷新
let refreshing: Promise<string | null> | null = null

async function refreshAccessToken(): Promise<string | null> {
  const observedRefreshToken = getRefreshToken()
  if (!observedRefreshToken) return null

  const rotate = async (): Promise<string | null> => {
    const currentRefreshToken = getRefreshToken()
    if (!currentRefreshToken) return null
    // Another tab may have completed rotation while this tab waited for the browser-wide lock.
    if (currentRefreshToken !== observedRefreshToken) return getAccessToken()

    try {
      // Use bare axios to avoid recursively entering this instance's interceptors.
      const { data } = await axios.post(
        '/api/auth/refresh',
        { refresh_token: currentRefreshToken },
        { timeout: 15_000 },
      )
      setTokens(data.access_token, data.refresh_token)
      return data.access_token as string
    } catch (error) {
      // A concurrent tab can win rotation just before a stale request receives its 401.
      if (getRefreshToken() !== currentRefreshToken) return getAccessToken()
      const status = (error as AxiosError).response?.status
      if (status === 401 || status === 403) {
        // Browsers without Web Locks can still receive the losing response first. Briefly allow
        // the winning tab's atomic storage event to arrive before discarding the shared session.
        await new Promise((resolve) => setTimeout(resolve, 200))
        if (getRefreshToken() !== currentRefreshToken) return getAccessToken()
        clearTokens()
        return null
      }
      // Network failures and 5xx responses do not prove that the durable session is invalid.
      throw error
    }
  }

  if (typeof navigator !== 'undefined' && navigator.locks) {
    return navigator.locks.request('prts-auth-refresh', rotate)
  }
  return rotate()
}

/** Clear an invalid session and send browser users back to the sign-in route. */
function handleRefreshFailure() {
  if (getAccessToken() || getRefreshToken()) clearTokens()
  if (typeof location !== 'undefined' && !location.hash.startsWith('#/login')) {
    location.hash = '#/login'
  }
}

/**
 * Authenticated Fetch transport for streaming responses.
 *
 * It shares the axios interceptor's single-flight refresh promise, while deliberately leaving
 * request lifetime to the caller's AbortSignal instead of the 15-second JSON request timeout.
 */
export async function authenticatedFetch(path: string, init: RequestInit = {}): Promise<Response> {
  const send = (token: string | null) => {
    const headers = new Headers(init.headers)
    headers.set('Accept-Language', i18n.global.locale.value)
    if (token) headers.set('Authorization', `Bearer ${token}`)
    return fetch(path, { ...init, headers })
  }

  let response = await send(getAccessToken())
  if (response.status !== 401 || init.signal?.aborted) return response
  if (!getRefreshToken()) {
    if (getAccessToken()) handleRefreshFailure()
    return response
  }

  if (!refreshing) {
    refreshing = refreshAccessToken().finally(() => {
      refreshing = null
    })
  }
  let token: string | null
  try {
    token = await refreshing
  } catch {
    // Temporary refresh failures leave the durable session intact for a later retry.
    return response
  }
  if (!token) {
    handleRefreshFailure()
    return response
  }
  await response.body?.cancel()
  response = await send(token)
  return response
}

http.interceptors.response.use(
  (resp) => resp,
  async (error: AxiosError) => {
    const original = error.config as (InternalAxiosRequestConfig & { _retry?: boolean }) | undefined
    if (error.response?.status === 401 && original && !original._retry) {
      original._retry = true
      if (!getRefreshToken()) {
        if (getAccessToken()) handleRefreshFailure()
        return Promise.reject(error)
      }
      if (!refreshing) {
        refreshing = refreshAccessToken().finally(() => {
          refreshing = null
        })
      }
      let token: string | null
      try {
        token = await refreshing
      } catch (refreshError) {
        return Promise.reject(refreshError)
      }
      if (token) {
        original.headers.Authorization = `Bearer ${token}`
        return http(original)
      }
      // 刷新失败：清理并回登录页（hash 路由）
      handleRefreshFailure()
    }
    return Promise.reject(error)
  },
)

/** 从 axios 错误中提取后端的本地化消息。 */
export function apiErrorMessage(
  error: unknown,
  fallback = i18n.global.t('common.requestFailed'),
): string {
  const e = error as AxiosError<{ message?: string; code?: string }>
  return e?.response?.data?.message || e?.message || fallback
}
