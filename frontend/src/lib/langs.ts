// 常用 BCP-47 语言（可在选择器里自定义补充）。显示名交给浏览器按当前界面语言生成。
export const COMMON_LANGS = [
  'en',
  'ja',
  'ko',
  'zh-Hans',
  'zh-Hant',
  'fr',
  'de',
  'es',
  'ru',
  'it',
  'pt',
  'vi',
  'th',
  'ar',
]

const displayNames = new Map<string, Intl.DisplayNames>()

/** 使用当前界面 locale 显示语言名称，同时保留 canonical BCP-47 code。 */
export function langLabel(code: string, locale: string): string {
  try {
    let formatter = displayNames.get(locale)
    if (!formatter) {
      formatter = new Intl.DisplayNames([locale], { type: 'language' })
      displayNames.set(locale, formatter)
    }
    const name = formatter.of(code)
    return name && name !== code ? `${name} · ${code}` : code
  } catch {
    return code
  }
}
