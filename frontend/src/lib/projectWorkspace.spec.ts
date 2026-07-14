import { describe, expect, it } from 'vitest'

import { hasProjectCapability } from './capabilities'
import { PROJECT_WORKSPACE_SECTIONS } from './projectWorkspace'

describe('project workspace navigation', () => {
  it('keeps the fixed section order with terminology enabled', () => {
    expect(PROJECT_WORKSPACE_SECTIONS.map((section) => section.key)).toEqual([
      'info',
      'files',
      'tasks',
      'terms',
      'leaderboard',
      'download',
      'manage',
    ])
    expect(
      PROJECT_WORKSPACE_SECTIONS.filter((section) => 'pending' in section).map(
        (section) => section.key,
      ),
    ).toEqual([])
    expect(PROJECT_WORKSPACE_SECTIONS.find((section) => section.key === 'terms')?.route).toBe(
      'project-terms',
    )
  })

  it('does not infer management visibility from a role name', () => {
    expect(hasProjectCapability(undefined, 'manage_project')).toBe(false)
    expect(
      hasProjectCapability(
        {
          view_project: true,
          manage_project: true,
          manage_members: false,
          upload_files: false,
          view_file_history: true,
          rollback_file_history: true,
          manage_tasks: true,
          manage_terms: true,
          download: false,
          edit_entry: false,
          review_entry: false,
          edit_locked_entry: false,
          force_save_presence: false,
          resolve_languages: false,
          change_primary_source: false,
          delete_project: false,
        },
        'manage_project',
      ),
    ).toBe(true)
  })
})
