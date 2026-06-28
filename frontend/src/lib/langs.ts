// 常用 BCP-47 语言（可在选择器里自定义补充）。

export const LANG_LABELS: Record<string, string> = {
  en: '英语',
  ja: '日语',
  ko: '韩语',
  'zh-Hans': '简体中文',
  'zh-Hant': '繁体中文',
  fr: '法语',
  de: '德语',
  es: '西班牙语',
  ru: '俄语',
  it: '意大利语',
  pt: '葡萄牙语',
  vi: '越南语',
  th: '泰语',
  ar: '阿拉伯语',
}

export const COMMON_LANGS = Object.keys(LANG_LABELS)

export function langLabel(code: string): string {
  const name = LANG_LABELS[code]
  return name ? `${name} · ${code}` : code
}
