import { describe, expect, it } from 'vitest'

import en from '@/i18n/locales/en.json'
import zhCn from '@/i18n/locales/zh-CN.json'
import apiSource from '@/api/index.ts?raw'
import apiTypesSource from '@/api/types.ts?raw'
import appSource from '@/App.vue?raw'
import authStoreSource from '@/stores/auth.ts?raw'
import adminSource from '@/views/AdminView.vue?raw'
import profileSource from '@/views/ProfileView.vue?raw'
import projectManageSource from '@/views/project/ProjectManageView.vue?raw'

import { hasPlatformCapability, hasProjectCapability } from './capabilities'

describe('hasProjectCapability', () => {
  it('uses only explicit API capability values', () => {
    expect(hasProjectCapability(undefined, 'manage_project')).toBe(false)
    expect(
      hasProjectCapability(
        {
          view_project: true,
          manage_project: false,
          manage_members: false,
          member_assignable_roles: [],
          upload_files: false,
          view_file_history: true,
          rollback_file_history: false,
          manage_tasks: false,
          manage_terms: false,
          download: false,
          edit_entry: false,
          review_entry: false,
          lock_entry: false,
          hide_entry: false,
          edit_locked_entry: false,
          force_save_presence: false,
          collaborate: false,
          use_ai: false,
          resolve_languages: false,
          change_primary_source: false,
          delete_project: false,
        },
        'view_project',
      ),
    ).toBe(true)
    expect(hasProjectCapability(undefined, 'manage_tasks')).toBe(false)
    expect(hasProjectCapability(undefined, 'manage_terms')).toBe(false)
  })
})

describe('hasPlatformCapability', () => {
  it('uses explicit platform capability values without role-name inference', () => {
    expect(hasPlatformCapability(undefined, 'manage_pos')).toBe(false)
    expect(
      hasPlatformCapability(
        {
          access_admin: true,
          grant_platform_roles: false,
          manage_users: true,
          create_project: true,
          manage_pos: true,
        },
        'manage_pos',
      ),
    ).toBe(true)
  })
})

describe('stage 7.1 admin users and password reminder contracts', () => {
  it('keeps admin user actions capability-driven and exposes keyset list/create APIs', () => {
    for (const required of [
      'AdminUserDto',
      'AdminUserListResponse',
      'password_change_required',
      'manage_users',
      'assignable_roles',
    ]) {
      expect(apiTypesSource).toContain(required)
    }
    expect(apiSource).toContain('get<AdminUserListResponse>')
    expect(apiSource).toContain("'/admin/users'")
    expect(apiSource).toContain("http.post<AdminUserDto>('/admin/users'")
    for (const parameter of ['q', 'role', 'sort', 'after', 'limit']) {
      expect(apiTypesSource).toContain(`${parameter}?:`)
    }
    expect(apiTypesSource).toContain('initial_password')

    expect(adminSource).toContain('adminApi.listUsers')
    expect(adminSource).toContain('adminApi.createUser')
    expect(adminSource).toContain('next_after')
    expect(adminSource).toContain('capabilities.can_change_role')
    expect(adminSource).toContain('capabilities.assignable_roles')
    expect(adminSource).not.toContain('auth.role ===')
    expect(adminSource).not.toContain('platform_role ===')
    expect(adminSource).not.toContain('cp_tenths')
    expect(adminSource).not.toContain('console.')
    expect(adminSource).not.toContain('localStorage')
    expect(adminSource).not.toContain('sessionStorage')
  })

  it('persists a non-blocking App reminder and clears it through Profile password change', () => {
    expect(apiTypesSource).toContain('cp_tenths: number')
    expect(apiTypesSource).not.toContain('cp: number')
    expect(apiTypesSource).toContain('password_change_required: boolean')
    expect(apiSource).toContain("http.put('/me/password'")
    expect(apiSource).toContain('current_password')
    expect(apiSource).toContain('new_password')

    expect(authStoreSource).toContain('passwordChangeRequired')
    expect(authStoreSource).toContain('user.value?.password_change_required')
    expect(appSource).toContain('auth.passwordChangeRequired')
    expect(appSource).toContain("t('app.passwordChangeRequired')")
    expect(appSource).toContain("name: 'me'")
    expect(appSource).not.toContain('router.replace')

    expect(profileSource).toContain('usersApi.changePassword')
    expect(profileSource).toContain('currentPassword')
    expect(profileSource).toContain('newPassword')
    expect(profileSource).toContain('confirmPassword')
    expect(profileSource).toContain('auth.refreshMe()')
    expect(profileSource).not.toContain('console.')
  })

  it('keeps Chinese and English stage 7 user-facing copy synchronized', () => {
    expect(zhCn.app).toHaveProperty('passwordChangeRequired')
    expect(en.app).toHaveProperty('passwordChangeRequired')
    expect(Object.keys(zhCn.app).sort()).toEqual(Object.keys(en.app).sort())

    expect(zhCn.admin).toHaveProperty('users')
    expect(en.admin).toHaveProperty('users')
    expect(Object.keys((zhCn.admin as Record<string, unknown>).users ?? {}).sort()).toEqual(
      Object.keys((en.admin as Record<string, unknown>).users ?? {}).sort(),
    )
    expect(zhCn).toHaveProperty('profile.password')
    expect(en).toHaveProperty('profile.password')
    expect(
      Object.keys(
        ((zhCn as Record<string, unknown>).profile as Record<string, unknown>)?.password ?? {},
      ).sort(),
    ).toEqual(
      Object.keys(
        ((en as Record<string, unknown>).profile as Record<string, unknown>)?.password ?? {},
      ).sort(),
    )
  })
})

describe('stage 7.2 project membership authorization contracts', () => {
  it('renders membership controls exclusively from per-target capabilities', () => {
    for (const required of [
      'MemberCapabilities',
      'assignable_roles',
      'can_change_role',
      'can_remove',
    ]) {
      expect(apiTypesSource).toContain(required)
    }
    expect(projectManageSource).toContain('projectsApi.members')
    expect(projectManageSource).toContain('projectsApi.addMember')
    expect(projectManageSource).toContain('projectsApi.removeMember')
    expect(projectManageSource).toContain('member.capabilities.can_change_role')
    expect(projectManageSource).toContain('member.capabilities.can_remove')
    expect(projectManageSource).toContain('detail?.capabilities.member_assignable_roles')
    expect(projectManageSource).not.toContain("member.role === 'owner'")
    expect(projectManageSource).not.toContain("member.role === 'manager'")
    expect(projectManageSource).not.toContain('auth.role ===')
  })
})

describe('stage 7.3 delayed owner-only deletion contracts', () => {
  it('requires all client gates before requesting a server challenge', async () => {
    const dialogSource = await import('@/components/project/ProjectDeleteDialog.vue?raw').then(
      (module) => module.default,
    )
    for (const required of [
      'consequencesConfirmed',
      'slugInput',
      'slugInput.value === props.slug',
      'projectsApi.deleteChallenge',
      'projectsApi.scheduleDeletion',
      'waitingPeriod',
      'readonly',
    ]) {
      expect(dialogSource).toContain(required)
    }
    expect(zhCn.project.deletion.waitingPeriod).toContain('24')
    expect(en.project.deletion.waitingPeriod).toContain('24')
    expect(dialogSource.indexOf('consequencesConfirmed')).toBeLessThan(
      dialogSource.indexOf('projectsApi.deleteChallenge'),
    )
    expect(dialogSource.indexOf('slugInput.value === props.slug')).toBeLessThan(
      dialogSource.indexOf('projectsApi.deleteChallenge'),
    )
  })

  it('removes legacy controls and renders pending state from capabilities', () => {
    expect(projectManageSource).toContain('ProjectDeleteDialog')
    expect(projectManageSource).toContain('projectsApi.cancelDeletion')
    expect(projectManageSource).toContain('detail?.project.deletion_scheduled_at')
    expect(projectManageSource).not.toContain('LegacyProjectControls')
    expect(apiTypesSource).toContain('DeleteChallengeDto')
    expect(apiTypesSource).toContain('DeletionStatusDto')
  })
})
