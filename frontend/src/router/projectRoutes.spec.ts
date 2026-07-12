import { describe, expect, it } from 'vitest'

import { projectRoutes } from './projectRoutes'

describe('project routes', () => {
  it('redirects the workspace root to read-only information', () => {
    const workspace = projectRoutes[0]
    expect(workspace?.children?.[0]?.redirect).toEqual({ name: 'project-info' })
    expect(workspace?.children?.map((route) => route.name).filter(Boolean)).toEqual([
      'project-info',
      'project-files',
      'project-leaderboard',
      'project-download',
      'project-manage',
    ])
  })

  it('keeps the editor outside the workspace shell', () => {
    expect(projectRoutes[1]?.name).toBe('editor')
    expect(projectRoutes[0]?.children?.some((route) => route.name === 'editor')).toBe(false)
  })
})
