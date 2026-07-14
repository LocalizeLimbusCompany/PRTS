import { describe, expect, it } from 'vitest'

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
          upload_files: false,
          view_file_history: true,
          rollback_file_history: false,
          manage_tasks: false,
          manage_terms: false,
          download: false,
          edit_entry: false,
          review_entry: false,
          edit_locked_entry: false,
          force_save_presence: false,
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
          create_project: true,
          manage_pos: true,
        },
        'manage_pos',
      ),
    ).toBe(true)
  })
})
