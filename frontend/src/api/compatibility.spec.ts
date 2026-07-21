// @vitest-environment jsdom

import { describe, expect, it, vi } from 'vitest'

import { http, searchApi, type StructuredSearchRequest } from '@/api'
import { uploadsApi } from '@/api/uploads'
import uploadDialogSource from '@/components/project/UploadBatchDialog.vue?raw'
import apiSource from '@/api/index.ts?raw'
import editorSource from '@/views/EditorView.vue?raw'
import projectFilesSource from '@/views/project/ProjectFilesView.vue?raw'

describe('project workspace compatibility handoff', () => {
  it('keeps the new frontend search client on POST only', async () => {
    const request: StructuredSearchRequest = {
      query: 'needle',
      conditions: [],
      case_sensitive: false,
      scope: { type: 'all' },
      states: [],
      include_hidden: false,
      vector: false,
      limit: 50,
    }
    const post = vi.spyOn(http, 'post').mockResolvedValue({ data: { items: [], next_after: null } })
    const get = vi.spyOn(http, 'get')

    await searchApi.search(9, request)

    expect(post).toHaveBeenCalledWith('/projects/9/search', request)
    expect(get).not.toHaveBeenCalled()
    expect(editorSource).toContain('searchApi.search')
  })

  it('keeps the workspace upload client on upload-batches only', async () => {
    const response = {
      data: {
        id: 1,
        state: 'uploading',
        expires_at: '2026-07-15T00:00:00Z',
        files: [],
      },
    }
    const post = vi.spyOn(http, 'post').mockResolvedValue(response)

    await uploadsApi.createBatch(9, [{ path: 'folder/file.json', size: 16 }])

    expect(post).toHaveBeenCalledWith('/projects/9/upload-batches', {
      files: [{ path: 'folder/file.json', size: 16 }],
    })
    expect(projectFilesSource).toContain('UploadBatchDialog')
    expect(projectFilesSource).toContain(':folders="folders"')
    expect(uploadDialogSource).toContain('destinationFolderId')
    expect(uploadDialogSource).not.toContain('webkitdirectory')
    expect(projectFilesSource).not.toContain('entriesApi.upload')
    expect(apiSource).not.toContain('`/projects/${id}/upload`')
  })
})
