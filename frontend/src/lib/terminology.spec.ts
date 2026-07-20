import { describe, expect, it } from 'vitest'

import {
  POS_IMPORT_FIELDS,
  TERM_IMPORT_FIELDS,
  displayPosName,
  importFormatFromFileName,
  insertTermTranslation,
  segmentSourceTerms,
} from './terminology'

describe('terminology file contracts', () => {
  it('keeps the stable term and POS field order', () => {
    expect(TERM_IMPORT_FIELDS).toEqual([
      'source_lang',
      'source_text',
      'match_mode',
      'translation',
      'pos',
      'notes',
      'archived',
    ])
    expect(POS_IMPORT_FIELDS).toEqual(['name_zh_cn', 'name_en', 'sort_order'])
  })

  it('accepts only CSV and JSON import file names', () => {
    expect(importFormatFromFileName('terms.CSV')).toBe('csv')
    expect(importFormatFromFileName('terms.json')).toBe('json')
    expect(importFormatFromFileName('terms.txt')).toBeNull()
  })

  it('uses locale-first POS display fallback', () => {
    const bilingual = { name_zh_cn: '名词', name_en: 'Noun' }
    expect(displayPosName(bilingual, 'zh-CN')).toBe('名词')
    expect(displayPosName(bilingual, 'en-US')).toBe('Noun')
    expect(displayPosName({ name_zh_cn: null, name_en: 'Verb' }, 'zh-CN')).toBe('Verb')
    expect(displayPosName({ name_zh_cn: '动词', name_en: null }, 'en')).toBe('动词')
  })

  it('segments source terms case-sensitively with longest overlap winning', () => {
    const terms = [
      { id: 2, source_text: 'cat' },
      { id: 1, source_text: 'catalog' },
      { id: 3, source_text: 'Cat' },
    ]
    expect(segmentSourceTerms('catalog cat Cat', terms)).toEqual([
      { text: 'catalog', term: terms[1] },
      { text: ' ', term: null },
      { text: 'cat', term: terms[0] },
      { text: ' ', term: null },
      { text: 'Cat', term: terms[2] },
    ])
  })

  it('inserts a matched term translation at the current selection', () => {
    expect(insertTermTranslation('hello world', 6, 11, 'PRTS')).toEqual({
      value: 'hello PRTS',
      cursor: 10,
    })
    expect(insertTermTranslation('hello world', 5, 5, ' PRTS')).toEqual({
      value: 'hello PRTS world',
      cursor: 10,
    })
  })
})
