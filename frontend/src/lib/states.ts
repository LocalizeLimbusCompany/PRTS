export const STATE_ORDER = ['untranslated', 'translated', 'questioned', 'checked', 'reviewed']

type Translator = (key: string) => string

/** 词条状态显示只从当前 locale 的消息表读取。 */
export function stateLabel(s: string, t: Translator): string {
  return STATE_ORDER.includes(s) ? t(`project.states.${s}`) : s
}

const ROLE_KEYS = new Set([
  'owner',
  'manager',
  'reviewer',
  'translator',
  'super_admin',
  'admin',
  'maintainer',
])

/** 平台与项目角色显示只从当前 locale 的消息表读取。 */
export function roleLabel(r: string | null | undefined, t: Translator): string {
  if (!r) return ''
  return ROLE_KEYS.has(r) ? t(`roles.${r}`) : r
}
