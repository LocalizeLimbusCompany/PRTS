// frontend/src/lib/saveButton.spec.ts
import { describe, it, expect } from 'vitest'
import { computeSaveButton, advanceOf, type SaveCtx } from './saveButton'

const base: SaveCtx = {
  isMember: true,
  locked: false,
  canEditLocked: false,
  isManager: false,
  canReview: false,
  canEdit: true,
  state: 'translated',
  dirty: false,
  othersEditing: false,
}

describe('advanceOf', () => {
  it('maps the workflow', () => {
    expect(advanceOf('translated')).toMatchObject({ nextState: 'checked', perm: 'review' })
    expect(advanceOf('questioned')).toMatchObject({ nextState: 'translated', perm: 'edit' })
    expect(advanceOf('checked')).toMatchObject({ nextState: 'reviewed', perm: 'review' })
    expect(advanceOf('reviewed')).toBeNull()
    expect(advanceOf('untranslated')).toBeNull()
  })
})

describe('computeSaveButton', () => {
  it('disables for non-members', () => {
    expect(computeSaveButton({ ...base, isMember: false })).toMatchObject({ disabled: true, mode: 'none' })
  })
  it('disables on locked entry without lock permission', () => {
    expect(computeSaveButton({ ...base, locked: true, canEditLocked: false })).toMatchObject({ disabled: true })
  })
  it('dirty → primary save', () => {
    expect(computeSaveButton({ ...base, dirty: true })).toMatchObject({ mode: 'save', color: 'primary', disabled: false })
  })
  it('unchanged translated as reviewer → advance to checked (检查)', () => {
    expect(computeSaveButton({ ...base, canReview: true, state: 'translated' }))
      .toMatchObject({ mode: 'advance', labelKey: 'check', nextState: 'checked', disabled: false })
  })
  it('unchanged translated as translator → grayed (no review perm)', () => {
    expect(computeSaveButton({ ...base, canReview: false, state: 'translated' }))
      .toMatchObject({ mode: 'none', disabled: true })
  })
  it('unchanged questioned as translator → advance to translated (已翻译)', () => {
    expect(computeSaveButton({ ...base, canEdit: true, canReview: false, state: 'questioned' }))
      .toMatchObject({ mode: 'advance', labelKey: 'translated', nextState: 'translated', disabled: false })
  })
  it('unchanged checked as reviewer → advance to reviewed (审核)', () => {
    expect(computeSaveButton({ ...base, canReview: true, state: 'checked' }))
      .toMatchObject({ mode: 'advance', labelKey: 'review', nextState: 'reviewed' })
  })
  it('unchanged reviewed → grayed terminal', () => {
    expect(computeSaveButton({ ...base, canReview: true, state: 'reviewed' }))
      .toMatchObject({ mode: 'none', disabled: true })
  })
  it('others editing + manager + dirty → red force (uses draftState)', () => {
    expect(computeSaveButton({ ...base, othersEditing: true, isManager: true, canEditLocked: true, dirty: true }))
      .toMatchObject({ mode: 'force', color: 'negative', disabled: false, nextState: undefined })
  })
  it('others editing + manager + unchanged advanceable → red force advancing', () => {
    expect(computeSaveButton({ ...base, othersEditing: true, isManager: true, canEditLocked: true, canReview: true, state: 'translated' }))
      .toMatchObject({ mode: 'force', color: 'negative', disabled: false, nextState: 'checked' })
  })
  it('others editing + non-manager → grayed', () => {
    expect(computeSaveButton({ ...base, othersEditing: true, isManager: false, dirty: true }))
      .toMatchObject({ mode: 'none', disabled: true })
  })
})
