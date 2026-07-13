import { describe, expect, it } from 'vitest'

import type { FileChangeSetDto } from '@/api/types'

import { canRestoreFileChangeSet, fileHistoryChangedFields, fileHistoryTarget } from './fileHistory'

const deletion: FileChangeSetDto = {
  id: '76a470da-e1fe-4940-8b0c-c403c5a11289',
  file_id: 17,
  folder_id: null,
  actor_id: 2,
  operation: 'delete',
  path_snapshot: 'story/main.json',
  metadata: {},
  created_at: '2026-07-13T00:00:00Z',
  items: [],
}

describe('file history UI rules', () => {
  it('restores only retained deletion targets with their operation id', () => {
    expect(fileHistoryTarget(deletion)).toEqual({ kind: 'file', id: 17 })
    expect(canRestoreFileChangeSet(deletion)).toBe(true)
    expect(canRestoreFileChangeSet({ ...deletion, operation: 'move' })).toBe(false)
    expect(canRestoreFileChangeSet({ ...deletion, file_id: null })).toBe(false)
  })

  it('summarizes allowlisted changed fields without returning their contents', () => {
    expect(
      fileHistoryChangedFields({
        id: 1,
        entity_type: 'entry',
        entity_id: 3,
        operation: 'update',
        before: {
          original: { en: 'same source' },
          translation: 'secret before',
          state: 'translated',
          locked: false,
        },
        after: {
          original: { en: 'same source' },
          translation: 'secret after',
          state: 'reviewed',
          locked: false,
        },
        ordinal: 0,
        created_at: '2026-07-13T00:00:00Z',
      }),
    ).toEqual(['state', 'translation'])
  })
})
