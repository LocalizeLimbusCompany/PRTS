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
