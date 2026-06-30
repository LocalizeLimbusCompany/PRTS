# Spec B — Editor Save Logic + Realtime Presence Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the editor's save button context-aware (dirty→save, unchanged→advance workflow, terminal/no-perm→grayed, others-editing→manager force-save / others grayed) and show the editor's avatar on entries others are editing — frontend only.

**Architecture:** A pure `computeSaveButton(ctx)` function (unit-tested) decides the button's label/color/disabled/mode; `EditorView.vue` builds the ctx from existing role computeds + a new `dirty` flag and drives one button. List avatars resolve `editingMap[entry.id]` → `members` → `avatar_url`. No backend changes — state-transition permissions and the optimistic `version` lock are already enforced server-side by `update_entry`.

**Tech Stack:** Vue 3 + Quasar + Pinia + vue-i18n, vitest for the pure-function unit test.

**Authoritative spec:** [`docs/superpowers/specs/2026-06-30-editor-save-presence-design.md`](../specs/2026-06-30-editor-save-presence-design.md).

---

## Notes
- Work on a feature branch off `master` (the controller cuts it). Commit per task; push triggers CI via a PR (`on: pull_request`). CI's stable clippy is irrelevant here (frontend only); `pnpm lint` + `pnpm build` are the gates, plus `pnpm test` for the unit spec.
- Run pnpm from `frontend/`. Bash needs `dangerouslyDisableSandbox: true` for installs/builds.

## File structure
| Path | Action | Responsibility |
| --- | --- | --- |
| `frontend/src/lib/saveButton.ts` | create | pure `advanceOf` + `computeSaveButton` (no Vue/DOM deps) |
| `frontend/src/lib/saveButton.spec.ts` | create | vitest unit tests for the precedence table |
| `frontend/src/views/EditorView.vue` | modify | build ctx, drive the save button, list editor avatars |
| `frontend/src/i18n/locales/zh-CN.json`, `en.json` | modify | button labels + "正在编辑" tooltip |

---

## Task 1: Pure save-button decision module

**Files:**
- Create: `frontend/src/lib/saveButton.ts`
- Create: `frontend/src/lib/saveButton.spec.ts`

- [ ] **Step 1: Write the module.**

```ts
// frontend/src/lib/saveButton.ts
// 上下文感知保存按钮的纯决策逻辑（无 Vue/DOM 依赖，便于单测）。

/** 工作流推进目标：未改译文时「保存」按钮的下一步。 */
export interface AdvanceTarget {
  nextState: string
  /** i18n label key 后缀（editor.btn<Pascal>），见下表。 */
  labelKey: 'check' | 'review' | 'translated'
  /** 所需权限：'edit'=任何成员，'review'=校对/管理/拥有者。 */
  perm: 'edit' | 'review'
}

export function advanceOf(state: string): AdvanceTarget | null {
  switch (state) {
    case 'translated':
      return { nextState: 'checked', labelKey: 'check', perm: 'review' }
    case 'questioned':
      return { nextState: 'translated', labelKey: 'translated', perm: 'edit' }
    case 'checked':
      return { nextState: 'reviewed', labelKey: 'review', perm: 'review' }
    default:
      return null // reviewed（终态）/ untranslated（无可推进）
  }
}

export interface SaveCtx {
  isMember: boolean
  locked: boolean
  canEditLocked: boolean // owner/manager
  isManager: boolean // owner/manager（与 canEditLocked 同集合）
  canReview: boolean // owner/manager/reviewer
  canEdit: boolean // 任何成员（拥有 PROJECT_ENTRY_EDIT）
  state: string // 已保存状态
  dirty: boolean // 译文或下拉状态相对已保存值有变化
  othersEditing: boolean // 他人正在编辑此词条（在场）
}

export type SaveMode = 'save' | 'advance' | 'force' | 'none'

export interface SaveButton {
  /** i18n label key 后缀：'save' | 'force' | 'check' | 'review' | 'translated'。 */
  labelKey: string
  /** Quasar color：'primary' | 'negative'(红) | undefined(灰)。 */
  color?: string
  disabled: boolean
  mode: SaveMode
  /** 推进/强制推进时的目标状态；为空时调用方用 draftState。 */
  nextState?: string
}

function advanceAllowed(adv: AdvanceTarget | null, ctx: SaveCtx): boolean {
  if (!adv) return false
  return adv.perm === 'review' ? ctx.canReview : ctx.canEdit
}

/** 决定保存按钮形态。首个匹配的分支胜出（对应 spec §2.2 优先级表）。 */
export function computeSaveButton(ctx: SaveCtx): SaveButton {
  // 1. 无编辑权限（非成员 / 锁定且非 owner/manager）
  if (!ctx.isMember || (ctx.locked && !ctx.canEditLocked)) {
    return { labelKey: 'save', disabled: true, mode: 'none' }
  }

  const adv = advanceOf(ctx.state)
  const canAdvance = advanceAllowed(adv, ctx)
  const hasContent = ctx.dirty || canAdvance

  // 2/3. 他人正在编辑此词条
  if (ctx.othersEditing) {
    if (ctx.isManager) {
      return {
        labelKey: 'force',
        color: 'negative',
        disabled: !hasContent,
        mode: 'force',
        nextState: ctx.dirty ? undefined : adv?.nextState,
      }
    }
    return { labelKey: 'save', disabled: true, mode: 'none' }
  }

  // 4. 脏 → 普通保存
  if (ctx.dirty) {
    return { labelKey: 'save', color: 'primary', disabled: false, mode: 'save' }
  }

  // 5. 未改 + 可推进
  if (canAdvance && adv) {
    return {
      labelKey: adv.labelKey,
      color: 'primary',
      disabled: false,
      mode: 'advance',
      nextState: adv.nextState,
    }
  }

  // 6. 其余（终态 / 无推进权 / 未翻译且未改）→ 灰
  return { labelKey: 'save', disabled: true, mode: 'none' }
}
```

- [ ] **Step 2: Write the unit spec.**

```ts
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
```

- [ ] **Step 3: Run the spec.**

Run: `cd frontend && pnpm test -- saveButton` (vitest). Expected: all pass.
If `pnpm test` is not wired to vitest in this project, check `frontend/package.json` scripts; run the project's unit-test command. At minimum the file must typecheck under `pnpm build`.

- [ ] **Step 4: Commit.**

```bash
git add frontend/src/lib/saveButton.ts frontend/src/lib/saveButton.spec.ts
git commit -m "feat(editor): pure computeSaveButton decision logic + unit tests

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 2: Wire the context-aware save button into EditorView

**Files:**
- Modify: `frontend/src/views/EditorView.vue`
- Modify: `frontend/src/i18n/locales/zh-CN.json`, `frontend/src/i18n/locales/en.json`

- [ ] **Step 1: Read the current save area + role computeds** in `EditorView.vue` (the `save()` handler ~L198, the role computeds `myRole`/`canReview`/`canEditLocked`/`isMember`/`panelReadOnly` ~L40, the translation `q-input` + Save `q-btn` + `q-select draftState` ~L449, and `otherEditing()` ~L260). Confirm exact names before editing.

- [ ] **Step 2: Add the `dirty` flag + button computed.** In `<script setup>`:

```ts
import { computeSaveButton } from '../lib/saveButton'

const isManager = computed(() => ['owner', 'manager'].includes(myRole.value ?? ''))
const canEdit = computed(() => isMember.value) // 任何成员都有 PROJECT_ENTRY_EDIT

const dirty = computed(
  () =>
    !!selected.value &&
    (draft.value !== selected.value.translation || draftState.value !== selected.value.state),
)

const saveBtn = computed(() =>
  computeSaveButton({
    isMember: isMember.value,
    locked: selected.value?.locked === true,
    canEditLocked: canEditLocked.value,
    isManager: isManager.value,
    canReview: canReview.value,
    canEdit: canEdit.value,
    state: selected.value?.state ?? 'untranslated',
    dirty: dirty.value,
    othersEditing: selected.value ? otherEditing(selected.value.id) : false,
  }),
)
```

- [ ] **Step 3: Make `save()` accept a target state.** Change `save()` so it sends the workflow-advance state when the button is in advance/force mode, else the dropdown state:

```ts
async function save() {
  if (!selected.value || saveBtn.value.disabled) return
  const targetState = saveBtn.value.nextState ?? draftState.value
  saving.value = true
  try {
    const updated = await entriesApi.update(props.id, selected.value.id, {
      translation: draft.value,
      state: targetState,
      version: selected.value.version,
    })
    draftState.value = targetState // 推进后同步下拉
    applyUpdated(updated)
    $q.notify({ type: 'positive', message: '已保存', timeout: 900 })
    selectNext()
  } catch (e) {
    const err = e as { response?: { status?: number } }
    if (err.response?.status === 409) {
      $q.notify({ type: 'warning', message: '该词条已被他人修改，已刷新为最新' })
      const fresh = await entriesApi.get(props.id, selected.value.id)
      applyUpdated(fresh)
      select(fresh)
    } else {
      $q.notify({ type: 'negative', message: apiErrorMessage(e, '保存失败') })
    }
  } finally {
    saving.value = false
  }
}
```

(Keep whatever the existing `applyUpdated`/`apiErrorMessage`/`selectNext` helpers are — match the real names found in Step 1.)

- [ ] **Step 4: Drive the button from `saveBtn`.** Replace the Save `q-btn` with:

```vue
<q-btn
  unelevated
  no-caps
  :color="saveBtn.color"
  :text-color="saveBtn.color ? 'dark' : undefined"
  icon="save"
  :label="t('editor.btn_' + saveBtn.labelKey)"
  :loading="saving"
  :disable="saveBtn.disabled"
  @click="save"
>
  <q-tooltip v-if="saveBtn.mode === 'force'">{{ t('editor.forceHint') }}</q-tooltip>
  <q-tooltip v-else-if="saveBtn.disabled && saveBtn.mode === 'none' && othersEditingSelected">
    {{ t('editor.othersEditingHint') }}
  </q-tooltip>
</q-btn>
```

Add a helper `const othersEditingSelected = computed(() => selected.value ? otherEditing(selected.value.id) : false)`. Keep the `draftState` `q-select` as-is (it sets `draftState`, which feeds `dirty`).

- [ ] **Step 5: i18n.** Add to both locale files under `editor`:

```jsonc
// zh-CN.json
"btn_save": "保存", "btn_force": "强制保存", "btn_check": "检查", "btn_review": "审核", "btn_translated": "标记已翻译",
"forceHint": "他人正在编辑，强制保存（仍按版本冲突校验）", "othersEditingHint": "他人正在编辑此词条"
// en.json
"btn_save": "Save", "btn_force": "Force save", "btn_check": "Mark checked", "btn_review": "Approve", "btn_translated": "Mark translated",
"forceHint": "Someone is editing; force-save (still version-checked)", "othersEditingHint": "Someone is editing this entry"
```

- [ ] **Step 6: Verify.** Run: `cd frontend && pnpm lint && pnpm build`. Expected: clean. Manually (or note for verify): unchanged translated as reviewer shows 检查; dirty shows 保存; reviewed shows grayed.

- [ ] **Step 7: Commit.**

```bash
git add frontend/src/views/EditorView.vue frontend/src/i18n/locales/zh-CN.json frontend/src/i18n/locales/en.json
git commit -m "feat(editor): context-aware save button (advance/force/grayed) + i18n

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 3: Editor avatars on entries others are editing

**Files:**
- Modify: `frontend/src/views/EditorView.vue`

- [ ] **Step 1: Add a resolver** in `<script setup>` (mirrors how `otherEditing` reads `editingMap`):

```ts
function editorOf(entryId: number): MemberDto | null {
  const uid = editingMap.value[entryId]
  if (uid === undefined || uid === auth.user?.id) return null
  return members.value.find((m) => m.user_id === uid) ?? null
}
```

(Import `MemberDto` from `../api/types` if not already.)

- [ ] **Step 2: Replace the amber edit icon in the entry-row template** (the existing `<q-icon v-if="otherEditing(item.id)" name="edit" ...>`):

```vue
<template v-if="otherEditing(item.id)">
  <q-avatar v-if="editorOf(item.id)?.avatar_url" size="18px">
    <img :src="editorOf(item.id)!.avatar_url!" :alt="editorOf(item.id)!.username" />
    <q-tooltip>{{ editorOf(item.id)!.username }} · {{ editorOf(item.id)!.role }} · {{ t('editor.editingNow') }}</q-tooltip>
  </q-avatar>
  <q-avatar v-else-if="editorOf(item.id)" size="18px" color="amber" text-color="dark">
    {{ editorOf(item.id)!.username.charAt(0).toUpperCase() }}
    <q-tooltip>{{ editorOf(item.id)!.username }} · {{ editorOf(item.id)!.role }} · {{ t('editor.editingNow') }}</q-tooltip>
  </q-avatar>
  <q-icon v-else name="edit" size="13px" color="amber">
    <q-tooltip>{{ t('editor.editingNow') }}</q-tooltip>
  </q-icon>
</template>
```

- [ ] **Step 3: i18n.** Add `"editingNow": "正在编辑"` (zh-CN) / `"editingNow": "editing now"` (en) under `editor`.

- [ ] **Step 4: Verify.** Run: `cd frontend && pnpm lint && pnpm build`. Expected: clean.

- [ ] **Step 5: Commit.**

```bash
git add frontend/src/views/EditorView.vue frontend/src/i18n/locales/zh-CN.json frontend/src/i18n/locales/en.json
git commit -m "feat(editor): show editor avatar on entries others are editing

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Self-review (completed during planning)

**Spec coverage:** §2 save button → Task 1 (pure logic + tests) + Task 2 (wiring). §2.3 advance map → `advanceOf` (Task 1). §3 list avatars → Task 3. §4 force-save → covered by `computeSaveButton` rows 2/3 (Task 1) + the button color/tooltip (Task 2); no backend change. §5 structure/testing → `saveButton.ts` + `.spec.ts`. §6 files → all covered. §7 red lines → no backend/version/locked changes (save still sends `version`; locked still gated by `panelReadOnly`/`canEditLocked`).

**Placeholder scan:** none — all code blocks complete; helper names flagged "match real names found in Step 1" are explicit lookups, not placeholders.

**Type consistency:** `computeSaveButton`/`SaveCtx`/`SaveButton`/`advanceOf`/`AdvanceTarget` consistent across Task 1 and consumed unchanged in Task 2. `labelKey` values (`save`/`force`/`check`/`review`/`translated`) match the i18n keys `btn_<labelKey>` in Task 2 Step 5. `editorOf` returns `MemberDto | null` consumed in Task 3 template.

**Lookups for the implementer** (named, not placeholders): exact names of `applyUpdated`, `apiErrorMessage`, `selectNext`, `members`, `editingMap`, `auth`, `t` — all already exist in `EditorView.vue`; confirm in Task 2 Step 1.
