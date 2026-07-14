/** Stable terminology interchange field order shared by UI hints and tests. */
export const TERM_IMPORT_FIELDS = [
  'source_lang',
  'source_text',
  'translation',
  'pos',
  'notes',
  'archived',
] as const

/** Stable bilingual POS interchange field order. */
export const POS_IMPORT_FIELDS = ['name_zh_cn', 'name_en', 'sort_order'] as const

export type TerminologyDocumentFormat = 'csv' | 'json'

/** Infer the only supported import formats without reading file contents. */
export function importFormatFromFileName(name: string): TerminologyDocumentFormat | null {
  const extension = name.split('.').pop()?.toLowerCase()
  return extension === 'csv' || extension === 'json' ? extension : null
}

/** Render a POS name from explicit locale with the opposite language as fallback. */
export function displayPosName(
  pos: { name_zh_cn: string | null; name_en: string | null },
  locale: string,
): string {
  const chineseFirst = locale.toLowerCase().startsWith('zh')
  return (
    (chineseFirst ? pos.name_zh_cn : pos.name_en) ??
    (chineseFirst ? pos.name_en : pos.name_zh_cn) ??
    ''
  )
}
