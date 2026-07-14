import type { EntryState } from '@/api/types'

/** 智能按钮只消费后端下发的 capability，不接触角色名称。 */
export interface SaveContext {
  canEdit: boolean
  canReview: boolean
  canEditLocked: boolean
  canForcePresence: boolean
  locked: boolean
  state: EntryState
  /** 译文正文相对服务端快照变化。 */
  dirty: boolean
  /** 用户显式改变状态下拉；优先尊重所选状态。 */
  stateChanged: boolean
  presenceConflict: boolean
}

export type SaveMode = 'translate' | 'save' | 'check' | 'review' | 'force' | 'none'

export interface SaveButton {
  labelKey: 'translate' | 'save' | 'check' | 'review' | 'force'
  color?: 'primary' | 'negative'
  disabled: boolean
  mode: SaveMode
  /** 请求必须保存的目标状态；force 只覆盖 presence，不覆盖版本。 */
  targetState?: EntryState
}

const disabledButton: SaveButton = {
  labelKey: 'save',
  disabled: true,
  mode: 'none',
}

function ordinaryAction(ctx: SaveContext): SaveButton {
  if (ctx.stateChanged) {
    return {
      labelKey: 'save',
      color: 'primary',
      disabled: false,
      mode: 'save',
      targetState: ctx.state,
    }
  }
  if (ctx.dirty) {
    if (ctx.state === 'untranslated') {
      return {
        labelKey: 'translate',
        color: 'primary',
        disabled: false,
        mode: 'translate',
        targetState: 'translated',
      }
    }
    return {
      labelKey: 'save',
      color: 'primary',
      disabled: false,
      mode: 'save',
      targetState: ctx.state,
    }
  }
  if (ctx.canReview && ctx.state === 'translated') {
    return {
      labelKey: 'check',
      color: 'primary',
      disabled: false,
      mode: 'check',
      targetState: 'checked',
    }
  }
  if (ctx.canReview && ctx.state === 'checked') {
    return {
      labelKey: 'review',
      color: 'primary',
      disabled: false,
      mode: 'review',
      targetState: 'reviewed',
    }
  }
  return disabledButton
}

/** 根据 dirty/state/presence/capabilities 返回唯一智能动作。 */
export function computeSaveButton(ctx: SaveContext): SaveButton {
  if (!ctx.canEdit || (ctx.locked && !ctx.canEditLocked)) return disabledButton

  const ordinary = ordinaryAction(ctx)
  if (!ctx.presenceConflict) return ordinary
  if (!ctx.canForcePresence || ordinary.disabled || !ordinary.targetState) return disabledButton
  return {
    labelKey: 'force',
    color: 'negative',
    disabled: false,
    mode: 'force',
    targetState: ordinary.targetState,
  }
}
