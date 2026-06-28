import { defineStore } from 'pinia'
import { computed, ref } from 'vue'

import { authApi, usersApi, type UserDto } from '@/api'
import { clearTokens, getAccessToken, getRefreshToken, setTokens } from '@/api/session'

export const useAuthStore = defineStore('auth', () => {
  const user = ref<UserDto | null>(null)
  const ready = ref(false)

  const isAuthed = computed(() => user.value !== null)
  const role = computed(() => user.value?.platform_role ?? null)
  const isSuperAdmin = computed(() => role.value === 'super_admin')
  const isAdmin = computed(() => role.value === 'super_admin' || role.value === 'admin')
  const canCreateProject = computed(() =>
    ['super_admin', 'admin', 'maintainer'].includes(role.value ?? ''),
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

  async function restore() {
    if (getAccessToken()) {
      try {
        user.value = await usersApi.me()
      } catch {
        user.value = null
      }
    }
    ready.value = true
  }

  // 会话恢复只执行一次；路由守卫与启动均可 await。
  let restorePromise: Promise<void> | null = null
  function ensureReady(): Promise<void> {
    if (!restorePromise) restorePromise = restore()
    return restorePromise
  }

  return {
    user,
    ready,
    isAuthed,
    role,
    isSuperAdmin,
    isAdmin,
    canCreateProject,
    login,
    register,
    applyTokens,
    logout,
    refreshMe,
    ensureReady,
  }
})
