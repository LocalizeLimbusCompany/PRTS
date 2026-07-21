import { describe, expect, it } from 'vitest'

import type { FileDto, FolderDto } from '@/api/types'

import {
  buildProjectBrowserModel,
  filterProjectBrowserItems,
  projectFileProgress,
  projectFolderItem,
  sortProjectFileItems,
} from './projectFiles'

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
    questioned_count: 0,
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
    questioned_count: 0,
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
    expect(
      sortProjectFileItems([file, projectFolderItem(folder, files)], 'progress')[0]?.kind,
    ).toBe('folder')
  })

  it('builds all folder aggregates and descendant selections in one tree model', () => {
    const child: FolderDto = {
      id: 5,
      parent_id: 4,
      name: 'chapter',
      path: 'dialog/chapter',
      created_at: '2026-01-01T00:00:00Z',
    }
    const model = buildProjectBrowserModel([folder, child], files)
    const root = model.items.find((item) => item.kind === 'folder' && item.id === folder.id)
    const nested = model.items.find((item) => item.kind === 'folder' && item.id === child.id)

    expect(root?.entryCount).toBe(15)
    expect(nested?.entryCount).toBe(5)
    expect(model.descendantFileIds.get(folder.id)).toEqual([11, 12])
    expect(model.descendantFileIds.get(child.id)).toEqual([12])
  })

  it('stays bounded when malformed folder parents form a cycle', () => {
    const cyclic = [
      { ...folder, parent_id: 5 },
      { ...folder, id: 5, name: 'nested', path: 'dialog/nested', parent_id: 4 },
    ]
    const model = buildProjectBrowserModel(cyclic, [files[0]!])
    expect(model.descendantFileIds.get(4)).toEqual([11])
    expect(model.descendantFileIds.get(5)).toEqual([11])
  })

  it('builds and searches 20,000 files without rescanning paths for every folder', () => {
    const folderCount = 200
    const fileCount = 20_000
    let pathReads = 0
    const manyFolders = Array.from({ length: folderCount }, (_, index): FolderDto => ({
      id: index + 1,
      parent_id: null,
      name: `folder-${index}`,
      path: `folder-${index}`,
      created_at: '2026-01-01T00:00:00Z',
    }))
    const manyFiles = Array.from({ length: fileCount }, (_, index) => {
      const folderId = (index % folderCount) + 1
      const value: FileDto = {
        id: index + 1,
        folder_id: folderId,
        name: `file-${index}.json`,
        path: '',
        entry_count: 1,
        state_counts: { untranslated: index % 2 },
        questioned_count: 0,
        created_at: '2026-01-01T00:00:00Z',
        updated_at: '2026-01-02T00:00:00Z',
      }
      Object.defineProperty(value, 'path', {
        enumerable: true,
        get() {
          pathReads += 1
          return `folder-${folderId - 1}/file-${index}.json`
        },
      })
      return value
    })

    const model = buildProjectBrowserModel(manyFolders, manyFiles)
    const result = filterProjectBrowserItems(model.items, 'file-19999.json', null, 'all')
    expect(model.items).toHaveLength(folderCount + fileCount)
    expect(pathReads).toBe(fileCount)
    expect(result.map((item) => item.id)).toEqual([20_000])
  })
})
