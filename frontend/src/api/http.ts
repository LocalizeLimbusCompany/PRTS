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
  const rt = getRefreshToken()
  if (!rt) return null
  try {
    // 用裸 axios，避免触发本实例拦截器造成递归
    const { data } = await axios.post('/api/auth/refresh', { refresh_token: rt })
    setTokens(data.access_token, data.refresh_token)
    return data.access_token as string
  } catch {
    clearTokens()
    return null
  }
}

http.interceptors.response.use(
  (resp) => resp,
  async (error: AxiosError) => {
    const original = error.config as (InternalAxiosRequestConfig & { _retry?: boolean }) | undefined
    if (error.response?.status === 401 && original && !original._retry && getRefreshToken()) {
      original._retry = true
      if (!refreshing) {
        refreshing = refreshAccessToken().finally(() => {
          refreshing = null
        })
      }
      const token = await refreshing
      if (token) {
        original.headers.Authorization = `Bearer ${token}`
        return http(original)
      }
      // 刷新失败：清理并回登录页（hash 路由）
      clearTokens()
      if (!location.hash.startsWith('#/login')) {
        location.hash = '#/login'
      }
    }
    return Promise.reject(error)
  },
)

/** 从 axios 错误中提取后端的本地化消息。 */
export function apiErrorMessage(error: unknown, fallback = '请求失败'): string {
  const e = error as AxiosError<{ message?: string; code?: string }>
  return e?.response?.data?.message || e?.message || fallback
}
