// @vitest-environment jsdom

import { describe, expect, it, vi } from 'vitest'

import en from '@/i18n/locales/en.json'
import zhCn from '@/i18n/locales/zh-CN.json'
import { authApi, http } from '@/api'
import loginSource from '@/views/LoginView.vue?raw'
import registerSource from '@/views/RegisterView.vue?raw'

describe('public authentication configuration', () => {
  it('loads the public capability endpoint without exposing admin settings', async () => {
    const response = {
      data: {
        password_login_enabled: false,
        password_registration_enabled: false,
        oauth_providers: ['zoot'],
      },
    }
    const get = vi.spyOn(http, 'get').mockResolvedValue(response)

    await expect(authApi.config()).resolves.toEqual(response.data)
    expect(get).toHaveBeenCalledWith('/meta/auth-config')
  })

  it('gates password and provider controls on server capabilities', () => {
    expect(loginSource).toContain('passwordLoginEnabled')
    expect(loginSource).toContain('passwordRegistrationEnabled')
    expect(loginSource).toContain('oauth_providers.includes')
    expect(loginSource).toContain('authApi.config()')
    expect(registerSource).toContain('config?.password_registration_enabled')
    expect(registerSource).toContain('authApi.config()')
  })

  it('keeps authentication copy synchronized in Chinese and English', () => {
    expect(Object.keys(zhCn.auth).sort()).toEqual(Object.keys(en.auth).sort())
    expect(Object.keys(zhCn.auth.login).sort()).toEqual(Object.keys(en.auth.login).sort())
    expect(Object.keys(zhCn.auth.register).sort()).toEqual(Object.keys(en.auth.register).sort())
    expect(loginSource).not.toContain('label="用户名 / 邮箱"')
    expect(registerSource).not.toContain('label="用户名"')
  })
})
