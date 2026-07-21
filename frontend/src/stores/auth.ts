import { defineStore } from 'pinia'
import { computed, ref } from 'vue'

import { authApi, usersApi, type UserDto } from '@/api'
import {
  clearTokens,
  getAccessToken,
  getRefreshToken,
  onSessionChange,
  setTokens,
} from '@/api/session'
import { hasPlatformCapability } from '@/lib/capabilities'
import { useAiExplanationSessionStore } from '@/stores/aiExplanationSession'

export const useAuthStore = defineStore('auth', () => {
  const user = ref<UserDto | null>(null)
  const ready = ref(false)
  const sessionVersion = ref(0)

  const isAuthed = computed(() => user.value !== null)
  // A transient profile request failure must not make the router discard an intact token pair.
  const hasSession = computed(() => {
    void sessionVersion.value
    return getAccessToken() !== null || getRefreshToken() !== null
  })
  const passwordChangeRequired = computed(() => user.value?.password_change_required === true)
  const role = computed(() => user.value?.platform_role ?? null)
  const isSuperAdmin = computed(() =>
    hasPlatformCapability(user.value?.platform_capabilities, 'grant_platform_roles'),
  )
  const isAdmin = computed(() =>
    hasPlatformCapability(user.value?.platform_capabilities, 'access_admin'),
  )
  const canCreateProject = computed(() =>
    hasPlatformCapability(user.value?.platform_capabilities, 'create_project'),
  )
  const canManagePos = computed(() =>
    hasPlatformCapability(user.value?.platform_capabilities, 'manage_pos'),
  )
  const canManageUsers = computed(() =>
    hasPlatformCapability(user.value?.platform_capabilities, 'manage_users'),
  )

  async function login(username: string, password: string) {
    const res = await authApi.login({ username, password })
    setTokens(res.access_token, res.refresh_token)
    user.value = res.user
  }

  async function register(username: string, password: string, email?: string) {
    const res = await authApi.register({ username, password, email })
    setTokens(res.access_token, res.refresh_token)
    user.value = res.user
  }

  /** OAuth 回调：直接写入令牌并拉取资料。 */
  async function applyTokens(access: string, refresh: string) {
    setTokens(access, refresh)
    user.value = await usersApi.me()
  }

  async function logout() {
    useAiExplanationSessionStore().clearAll()
    const rt = getRefreshToken()
    if (rt) {
      try {
        await authApi.logout(rt)
      } catch {
        /* 忽略 */
      }
    }
    clearTokens()
    user.value = null
  }

  async function refreshMe() {
    user.value = await usersApi.me()
  }

  async function restore(): Promise<boolean> {
    let stable = true
    if (getAccessToken() || getRefreshToken()) {
      try {
        user.value = await usersApi.me()
      } catch {
        user.value = null
        // Invalid credentials are cleared by the HTTP layer. Preserved credentials indicate a
        // temporary transport/server failure, so restore it again without requiring navigation.
        stable = !getAccessToken() && !getRefreshToken()
        if (!stable) scheduleRestoreRetry()
      }
    }
    ready.value = true
    return stable
  }

  // 会话恢复只执行一次；路由守卫与启动均可 await。
  let restorePromise: Promise<void> | null = null
  let restoreRetryTimer: ReturnType<typeof setTimeout> | null = null

  /** Retry a preserved session after a transient transport or server failure. */
  function scheduleRestoreRetry() {
    if (restoreRetryTimer) return
    restoreRetryTimer = setTimeout(() => {
      restoreRetryTimer = null
      if (getAccessToken() || getRefreshToken()) void ensureReady()
    }, 1_000)
  }

  function ensureReady(): Promise<void> {
    if (!restorePromise) {
      restorePromise = restore().then((stable) => {
        if (!stable) restorePromise = null
      })
    }
    return restorePromise
  }

  // Keep Pinia state consistent when an interceptor or another tab invalidates the token pair.
  onSessionChange(() => {
    sessionVersion.value += 1
    if (getAccessToken() || getRefreshToken()) return
    if (restoreRetryTimer) clearTimeout(restoreRetryTimer)
    restoreRetryTimer = null
    useAiExplanationSessionStore().clearAll()
    user.value = null
  })

  return {
    user,
    ready,
    isAuthed,
    hasSession,
    passwordChangeRequired,
    role,
    isSuperAdmin,
    isAdmin,
    canCreateProject,
    canManagePos,
    canManageUsers,
    login,
    register,
    applyTokens,
    logout,
    refreshMe,
    ensureReady,
  }
})
