import { describe, expect, it } from 'vitest'

import type { FileDto, FolderDto } from '@/api/types'

import { descendantTaskFileIds, taskProgressPercent, toggleTaskFileSelection } from './projectTasks'
import fileBrowserSource from '@/components/project/ProjectFileBrowser.vue?raw'
import taskDetailSource from '@/views/project/tasks/TaskDetailView.vue?raw'

const folders: FolderDto[] = [
  { id: 1, parent_id: null, name: 'dir', path: 'dir', created_at: '2026-01-01T00:00:00Z' },
  { id: 2, parent_id: 1, name: 'child', path: 'dir/child', created_at: '2026-01-01T00:00:00Z' },
  { id: 3, parent_id: null, name: 'dir2', path: 'dir2', created_at: '2026-01-01T00:00:00Z' },
]

function file(id: number, folderId: number | null, path: string): FileDto {
  return {
    id,
    folder_id: folderId,
    name: path.split('/').at(-1) ?? path,
    path,
    entry_count: 0,
    state_counts: {},
    questioned_count: 0,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
  }
}

describe('project task workspace helpers', () => {
  it('expands folders through parent relationships without path-prefix collisions', () => {
    const files = [
      file(11, 1, 'dir/a.json'),
      file(12, 2, 'dir/child/b.json'),
      file(13, 3, 'dir2/c.json'),
    ]
    expect(descendantTaskFileIds(1, folders, files)).toEqual([11, 12])
    expect(descendantTaskFileIds(3, folders, files)).toEqual([13])
  })

  it('saves a deterministic complete desired file set', () => {
    expect(toggleTaskFileSelection([12, 20], [11, 12], true)).toEqual([11, 12, 20])
    expect(toggleTaskFileSelection([11, 12, 20], [11, 12], false)).toEqual([20])
  })

  it('renders an empty baseline as complete', () => {
    expect(taskProgressPercent(0, 0)).toBe(100)
    expect(taskProgressPercent(4, 3)).toBe(75)
  })

  it('virtualizes project and task file rows and keeps task files below the introduction', () => {
    expect(fileBrowserSource).toContain('<q-virtual-scroll')
    expect(fileBrowserSource).toContain('deferredQuery')
    expect(taskDetailSource).toContain('<q-virtual-scroll')
    expect(taskDetailSource).toContain('deferredFileQuery')
    expect(taskDetailSource.indexOf("$t('project.tasks.introduction')")).toBeLessThan(
      taskDetailSource.indexOf('class="task-detail__files"'),
    )
    expect(taskDetailSource).not.toContain('task-detail__grid')
  })
})
