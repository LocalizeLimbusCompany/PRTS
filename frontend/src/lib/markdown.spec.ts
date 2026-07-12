// @vitest-environment jsdom

import { describe, expect, it } from 'vitest'

import { renderSafeMarkdown } from './markdown'

describe('renderSafeMarkdown', () => {
  it('keeps Markdown formatting while removing executable content', () => {
    const output = renderSafeMarkdown(`
# 项目说明

**重要** [安全链接](https://example.com)

<script>alert('x')</script>

[危险链接](javascript:alert('x'))

<img src=x onerror="alert('x')">
`)

    expect(output).toContain('<h1>项目说明</h1>')
    expect(output).toContain('<strong>重要</strong>')
    expect(output).toContain('https://example.com')
    expect(output).not.toContain('<script')
    expect(output).not.toContain('javascript:')
    expect(output).not.toContain('onerror')
  })
})
