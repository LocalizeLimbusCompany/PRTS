// 词条状态的展示辅助。

export const STATE_LABELS: Record<string, string> = {
  untranslated: '未翻译',
  translated: '已翻译',
  questioned: '有疑问',
  checked: '已检查',
  reviewed: '已审核',
}

export const STATE_ORDER = ['untranslated', 'translated', 'questioned', 'checked', 'reviewed']

export function stateLabel(s: string): string {
  return STATE_LABELS[s] ?? s
}

/** 项目角色中文名。 */
export const ROLE_LABELS: Record<string, string> = {
  owner: '拥有者',
  manager: '管理',
  reviewer: '校对',
  translator: '翻译',
  super_admin: '总管理员',
  admin: '管理员',
  maintainer: '维护者',
}

export function roleLabel(r: string | null | undefined): string {
  return r ? (ROLE_LABELS[r] ?? r) : ''
}
