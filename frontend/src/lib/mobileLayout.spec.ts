import { describe, expect, it } from 'vitest'

import editorSource from '@/views/EditorView.vue?raw'
import manageSource from '@/views/project/ProjectManageView.vue?raw'
import projectShellSource from '@/views/project/ProjectShell.vue?raw'
import termsSource from '@/views/project/ProjectTermsView.vue?raw'

describe('mobile layout contracts', () => {
  it('keeps 360/390 editor controls bounded and icon-first', () => {
    expect(editorSource).toContain('@media (max-width: 420px)')
    expect(editorSource).toContain('.context-tabs :deep(.q-tab__label)')
    expect(editorSource).toContain('display: none')
    expect(editorSource).toContain('grid-template-columns: minmax(0, 1fr) 40px')
    expect(editorSource).toContain('overflow-wrap: anywhere')
  })

  it('provides safe narrow-screen table scrolling and flexible children', () => {
    expect(projectShellSource).toContain('overflow-x: auto')
    expect(editorSource).toContain('min-width: 0')
    expect(termsSource).toContain('.terms-view__table-wrap')
    expect(termsSource).toContain('overflow-x: auto')
  })

  it('keeps workspace and management navigation usable on narrow screens', () => {
    expect(projectShellSource).toContain('overflow-x: auto')
    expect(projectShellSource).toContain('text-overflow: ellipsis')
    expect(manageSource).toContain('@media (max-width: 420px)')
    expect(manageSource).toContain('.manage-view__tabs :deep(.q-tab__label)')
  })
})
