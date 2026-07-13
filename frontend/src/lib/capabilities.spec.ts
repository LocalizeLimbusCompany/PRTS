import { describe, expect, it } from 'vitest'

import { hasProjectCapability } from './capabilities'

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
  })
})
