import { describe, expect, it } from 'vitest'

import editorSource from '@/views/EditorView.vue?raw'

import { computeSaveButton, type SaveContext } from './saveButton'
import saveSource from './saveButton.ts?raw'

const base: SaveContext = {
  canEdit: true,
  canReview: false,
  canEditLocked: false,
  canForcePresence: false,
  locked: false,
  state: 'translated',
  dirty: false,
  stateChanged: false,
  presenceConflict: false,
}

describe('computeSaveButton', () => {
  it('dirty untranslated translates instead of preserving untranslated', () => {
    expect(computeSaveButton({ ...base, state: 'untranslated', dirty: true })).toEqual({
      mode: 'translate',
      labelKey: 'translate',
      color: 'primary',
      disabled: false,
      targetState: 'translated',
    })
  })

  it.each(['translated', 'checked', 'reviewed'] as const)(
    'dirty %s saves while preserving the current state',
    (state) => {
      expect(computeSaveButton({ ...base, state, dirty: true })).toEqual({
        mode: 'save',
        labelKey: 'save',
        color: 'primary',
        disabled: false,
        targetState: state,
      })
    },
  )

  it('preserves an explicitly selected untranslated state instead of auto-translating it', () => {
    expect(
      computeSaveButton({
        ...base,
        state: 'untranslated',
        dirty: false,
        stateChanged: true,
      }),
    ).toMatchObject({ mode: 'save', targetState: 'untranslated' })
  })

  it('clean translated checks only with review_entry capability', () => {
    expect(computeSaveButton({ ...base, state: 'translated', canReview: true })).toEqual({
      mode: 'check',
      labelKey: 'check',
      color: 'primary',
      disabled: false,
      targetState: 'checked',
    })
    expect(computeSaveButton({ ...base, state: 'translated', canReview: false })).toMatchObject({
      mode: 'none',
      disabled: true,
    })
  })

  it('clean checked reviews only with review_entry capability', () => {
    expect(computeSaveButton({ ...base, state: 'checked', canReview: true })).toEqual({
      mode: 'review',
      labelKey: 'review',
      color: 'primary',
      disabled: false,
      targetState: 'reviewed',
    })
    expect(computeSaveButton({ ...base, state: 'checked', canReview: false })).toMatchObject({
      mode: 'none',
      disabled: true,
    })
  })

  it('presence conflict exposes force only through force_save_presence capability', () => {
    expect(
      computeSaveButton({
        ...base,
        state: 'untranslated',
        dirty: true,
        presenceConflict: true,
        canForcePresence: true,
      }),
    ).toEqual({
      mode: 'force',
      labelKey: 'force',
      color: 'negative',
      disabled: false,
      targetState: 'translated',
    })
    expect(
      computeSaveButton({
        ...base,
        dirty: true,
        presenceConflict: true,
        canForcePresence: false,
      }),
    ).toMatchObject({ mode: 'none', disabled: true })
  })

  it.each([
    { canEdit: false },
    { locked: true, canEditLocked: false },
    { state: 'untranslated' as const, dirty: false },
    { state: 'reviewed' as const, dirty: false, canReview: true },
  ])('disables non-actionable context %#', (overrides) => {
    expect(computeSaveButton({ ...base, ...overrides })).toMatchObject({
      mode: 'none',
      disabled: true,
    })
  })

  it('contains no role-name inference in the smart action or editor', () => {
    expect(saveSource).not.toContain('isManager')
    expect(editorSource).not.toContain('const myRole')
    expect(editorSource).not.toMatch(/\[['"]owner['"],\s*['"]manager['"]\]/)
  })
})
