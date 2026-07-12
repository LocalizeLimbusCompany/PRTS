import { describe, expect, it } from 'vitest'

import type { FileDto, FolderDto } from '@/api/types'

import { projectFileProgress, projectFolderItem, sortProjectFileItems } from './projectFiles'

const folder: FolderDto = {
  id: 4,
  parent_id: null,
  name: 'dialog',
  path: 'dialog',
  created_at: '2026-01-01T00:00:00Z',
}

const files: FileDto[] = [
  {
    id: 11,
    folder_id: 4,
    name: 'a.json',
    path: 'dialog/a.json',
    entry_count: 10,
    state_counts: { untranslated: 4, translated: 6 },
    created_at: '2026-01-02T00:00:00Z',
    updated_at: '2026-01-03T00:00:00Z',
  },
  {
    id: 12,
    folder_id: 5,
    name: 'b.json',
    path: 'dialog/chapter/b.json',
    entry_count: 5,
    state_counts: { untranslated: 1, reviewed: 4 },
    created_at: '2026-01-02T00:00:00Z',
    updated_at: '2026-01-05T00:00:00Z',
  },
]

describe('project file browser model', () => {
  it('aggregates every descendant file from materialized counts', () => {
    const item = projectFolderItem(folder, files)
    expect(item.entryCount).toBe(15)
    expect(item.stateCounts).toMatchObject({ untranslated: 5, translated: 6, reviewed: 4 })
    expect(item.updatedAt).toBe('2026-01-05T00:00:00Z')
    expect(projectFileProgress(item)).toBeCloseTo(2 / 3)
  })

  it('uses the folder creation time and an empty progress for an empty folder', () => {
    const item = projectFolderItem(folder, [])
    expect(item.updatedAt).toBe(folder.created_at)
    expect(projectFileProgress(item)).toBeNull()
  })

  it('keeps folders before files for every requested sort', () => {
    const file = {
      ...projectFolderItem(folder, files),
      id: 99,
      kind: 'file' as const,
      name: 'z.json',
    }
    expect(sortProjectFileItems([file, projectFolderItem(folder, files)], 'progress')[0]?.kind).toBe(
      'folder',
    )
  })
})
