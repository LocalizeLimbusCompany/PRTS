import { describe, expect, it } from 'vitest'

import { availableProjectManageTabs, resolveProjectManageTab } from './projectManageTabs'

describe('project management tab access', () => {
  it('shows all five sections to an active project owner', () => {
    expect(
      availableProjectManageTabs({
        manageProject: true,
        owner: true,
        manageMembers: true,
        deleteProject: true,
        deletionPending: false,
      }),
    ).toEqual(['basic', 'ai', 'language', 'members', 'danger'])
  })

  it('hides owner-only sections and falls back from an inaccessible query', () => {
    const available = availableProjectManageTabs({
      manageProject: true,
      owner: false,
      manageMembers: true,
      deleteProject: false,
      deletionPending: false,
    })
    expect(available).toEqual(['basic', 'language', 'members'])
    expect(resolveProjectManageTab('ai', available)).toBe('basic')
  })

  it('keeps only cancellation controls while deletion is pending', () => {
    const available = availableProjectManageTabs({
      manageProject: false,
      owner: true,
      manageMembers: false,
      deleteProject: true,
      deletionPending: true,
    })
    expect(available).toEqual(['danger'])
    expect(resolveProjectManageTab('basic', available)).toBe('danger')
  })
})
