import { readFileSync } from 'node:fs'

import { describe, expect, it } from 'vitest'

describe('streaming upload client contract', () => {
  it('never parses selected JSON files in the browser', () => {
    const source = readFileSync(new URL('./useUploadBatch.ts', import.meta.url), 'utf8')

    expect(source).not.toContain('.text(')
    expect(source).not.toContain('JSON.parse')
    expect(source).toContain('receiveAttempt')
  })
})
