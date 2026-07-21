// @vitest-environment jsdom

import { afterEach, describe, expect, it, vi } from 'vitest'

import { http, searchApi, type StructuredSearchRequest } from '@/api'
import { projectRoutes } from '@/router/projectRoutes'
import { canOpenPresenceMenu, shouldConnectProjectRealtime } from '@/composables/useRealtime'
import en from '@/i18n/locales/en.json'
import zhCn from '@/i18n/locales/zh-CN.json'
import editorSource from '@/views/EditorView.vue?raw'

import { buildAdvancedSearchRequest } from './AdvancedFilterDialog.vue'
import { quickSearchRequest } from './SearchBar.vue'

afterEach(() => vi.restoreAllMocks())

describe('editor structured search workflow', () => {
  it('ignores Enter while IME is composing', () => {
    expect(quickSearchRequest('正文', false, true, 41)).toBeNull()
  })

  it('maps Enter to all and Shift+Enter to the explicit current file id', () => {
    expect(quickSearchRequest(' weather ', false, false, 41)).toMatchObject({
      query: 'weather',
      scope: { type: 'all' },
      vector: false,
    })
    expect(quickSearchRequest(' weather ', true, false, 41)).toMatchObject({
      query: 'weather',
      scope: { type: 'current_file', file_id: 41 },
      vector: false,
    })
    expect(quickSearchRequest('weather', true, false, null)).toBeNull()
  })

  it.each([
    [{ scopeType: 'all' as const }, { type: 'all' }],
    [
      { scopeType: 'path' as const, path: 'chapter/01' },
      { type: 'path', path: 'chapter/01' },
    ],
    [
      { scopeType: 'file' as const, fileId: 41 },
      { type: 'file', file_id: 41 },
    ],
    [
      { scopeType: 'current_file' as const, currentFileId: 41 },
      { type: 'current_file', file_id: 41 },
    ],
    [
      { scopeType: 'current_task' as const, taskId: 73 },
      { type: 'current_task', task_id: 73 },
    ],
  ])('builds exact advanced scope payload %#', (scopeDraft, scope) => {
    expect(
      buildAdvancedSearchRequest({
        query: 'needle',
        conditions: [{ field: 'translation', operator: 'contains', value: 'x' }],
        states: ['translated', 'checked'],
        questioned: false,
        includeHidden: true,
        vector: false,
        caseSensitive: false,
        mode: 'normal',
        ...scopeDraft,
      }),
    ).toEqual({
      query: 'needle',
      conditions: [{ field: 'translation', operator: 'contains', value: 'x' }],
      case_sensitive: false,
      scope,
      states: ['translated', 'checked'],
      include_hidden: true,
      vector: false,
      limit: 50,
    })
  })

  it('uses POST structured search and preserves the next_after envelope', async () => {
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
    const response = { items: [], next_after: 'signed.cursor', total_items: 121 }
    const post = vi.spyOn(http, 'post').mockResolvedValue({ data: response })
    await expect(searchApi.search(7, request)).resolves.toEqual(response)
    expect(post).toHaveBeenCalledWith('/projects/7/search', request)
  })
})

describe('guest and presence boundaries', () => {
  it('keeps the public editor route available without authentication', () => {
    const editor = projectRoutes.find((route) => route.name === 'editor')
    expect(editor?.meta?.requiresAuth).not.toBe(true)
  })

  it('does not connect project realtime for guests or non-collaborating viewers', () => {
    expect(shouldConnectProjectRealtime(false, false)).toBe(false)
    expect(shouldConnectProjectRealtime(true, false)).toBe(false)
    expect(shouldConnectProjectRealtime(true, true)).toBe(true)
  })

  it('shows own presence but never opens poke or DM actions for self', () => {
    expect(canOpenPresenceMenu(5, 5, true)).toBe(false)
    expect(canOpenPresenceMenu(6, 5, false)).toBe(false)
    expect(canOpenPresenceMenu(6, 5, true)).toBe(true)
    expect(editorSource).toContain('if (auth.user?.id === userId)')
    expect(editorSource).toContain('canOpenPresenceMenu(')
    expect(editorSource).toContain('target.user_id')
  })
})

describe('entry history refresh workflow', () => {
  it('reloads authoritative history after a successful local save', () => {
    expect(editorSource).toContain('async function refreshEntryHistory(entryId: number)')
    expect(editorSource).toMatch(
      /applyUpdated\(updated\)\s+if \(selected\.value\?\.id === entryId\) await refreshEntryHistory\(entryId\)/,
    )
  })

  it('refreshes the selected history after realtime updates from another editor', () => {
    expect(editorSource).toContain(
      'if (selected.value?.id === fresh.id) void refreshEntryHistory(fresh.id)',
    )
  })
})

describe('editor locale contract', () => {
  it('keeps Chinese and English editor/common action keys synchronized', () => {
    expect(Object.keys(zhCn.editor).sort()).toEqual(Object.keys(en.editor).sort())
    expect(Object.keys(zhCn.common).sort()).toEqual(Object.keys(en.common).sort())
  })
})
