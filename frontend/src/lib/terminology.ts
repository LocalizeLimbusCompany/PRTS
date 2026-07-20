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

export interface TermMatchLike {
  id: number
  source_text: string
}

export interface TermTextSegment<T extends TermMatchLike = TermMatchLike> {
  text: string
  term: T | null
}

/** Insert a term translation at a textarea selection without saving the entry. */
export function insertTermTranslation(
  value: string,
  selectionStart: number,
  selectionEnd: number,
  insertion: string,
): { value: string; cursor: number } {
  const start = Math.max(0, Math.min(selectionStart, value.length))
  const end = Math.max(start, Math.min(selectionEnd, value.length))
  return {
    value: value.slice(0, start) + insertion + value.slice(end),
    cursor: start + insertion.length,
  }
}

/** Case-sensitive, longest-first, non-overlapping term segmentation for source text rendering. */
export function segmentSourceTerms<T extends TermMatchLike>(
  source: string,
  terms: T[],
): TermTextSegment<T>[] {
  if (!source || terms.length === 0) return source ? [{ text: source, term: null }] : []
  const candidates = [...terms]
    .filter((term) => term.source_text.length > 0)
    .sort((left, right) => right.source_text.length - left.source_text.length || left.id - right.id)
  const result: TermTextSegment<T>[] = []
  let plainStart = 0
  let cursor = 0
  while (cursor < source.length) {
    const term = candidates.find((candidate) => source.startsWith(candidate.source_text, cursor))
    if (!term) {
      cursor += 1
      continue
    }
    if (plainStart < cursor) result.push({ text: source.slice(plainStart, cursor), term: null })
    result.push({ text: term.source_text, term })
    cursor += term.source_text.length
    plainStart = cursor
  }
  if (plainStart < source.length) result.push({ text: source.slice(plainStart), term: null })
  return result
}
