import { describe, expect, it } from 'vitest'

import { resolveProfileTab } from './profileTabs'

describe('profile tab routing', () => {
  it.each(['profile', 'ai', 'security', 'api_keys'] as const)('accepts %s', (tab) => {
    expect(resolveProfileTab(tab)).toBe(tab)
  })

  it('falls back for missing, repeated, and unknown values', () => {
    expect(resolveProfileTab(undefined)).toBe('profile')
    expect(resolveProfileTab(['ai', 'security'])).toBe('profile')
    expect(resolveProfileTab('unknown')).toBe('profile')
  })
})
