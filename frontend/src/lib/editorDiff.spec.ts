import { describe, expect, it } from 'vitest'

import { diffText } from './editorDiff'

describe('entry history diff', () => {
  it('preserves equal text and marks insertions/deletions', () => {
    expect(diffText('hello world', 'hello brave world', 'word')).toEqual([
      { kind: 'equal', text: 'hello ' },
      { kind: 'insert', text: 'brave ' },
      { kind: 'equal', text: 'world' },
    ])
  })

  it('supports character-level CJK differences', () => {
    expect(diffText('旧原文', '新原文', 'character')).toEqual([
      { kind: 'delete', text: '旧' },
      { kind: 'insert', text: '新' },
      { kind: 'equal', text: '原文' },
    ])
  })

  it('keeps long history diffs bounded while preserving shared edges', () => {
    const before = `prefix ${'old '.repeat(600)}suffix`
    const after = `prefix ${'new '.repeat(600)}suffix`
    const parts = diffText(before, after, 'word')

    expect(parts[0]).toEqual({ kind: 'equal', text: 'prefix ' })
    expect(parts.at(-1)).toEqual({ kind: 'equal', text: ' suffix' })
    expect(parts.some((part) => part.kind === 'delete')).toBe(true)
    expect(parts.some((part) => part.kind === 'insert')).toBe(true)
  })
})
