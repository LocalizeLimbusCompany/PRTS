import { describe, expect, it } from 'vitest'

import en from '@/i18n/locales/en.json'
import zhCn from '@/i18n/locales/zh-CN.json'
import profileSource from '@/views/ProfileView.vue?raw'

import { langLabel } from '@/lib/langs'
import { roleLabel, stateLabel } from '@/lib/states'

const applicationSources = import.meta.glob(['/src/**/*.ts', '/src/**/*.vue'], {
  eager: true,
  query: '?raw',
  import: 'default',
}) as Record<string, string>

function scalarPaths(value: unknown, prefix = ''): string[] {
  if (value === null || typeof value !== 'object') return [prefix]
  return Object.entries(value).flatMap(([key, child]) =>
    scalarPaths(child, prefix ? `${prefix}.${key}` : key),
  )
}

/** 注释允许使用中文；运行时代码与模板文案必须通过消息表或本地化数据生成。 */
function stripComments(source: string): string {
  return source
    .replace(/<!--[\s\S]*?-->/g, '')
    .replace(/\/\*[\s\S]*?\*\//g, '')
    .replace(/\/\/.*$/gm, '')
}

describe('frontend localization contracts', () => {
  it('keeps every Chinese and English scalar message key synchronized', () => {
    expect(scalarPaths(zhCn).sort()).toEqual(scalarPaths(en).sort())
  })

  it('does not ship hard-coded Chinese user copy outside locale files', () => {
    for (const [path, rawSource] of Object.entries(applicationSources)) {
      if (path.includes('/i18n/locales/') || path.endsWith('.spec.ts')) continue
      expect(stripComments(rawSource), path).not.toMatch(/\p{Script=Han}/u)
    }
  })

  it('localizes language, workflow-state and role labels from the active locale', () => {
    expect(langLabel('en', 'en')).toContain('English')
    expect(langLabel('en', 'zh-CN')).toContain('en')
    expect(stateLabel('translated', (key) => `translated:${key}`)).toBe(
      'translated:project.states.translated',
    )
    expect(roleLabel('reviewer', (key) => `reviewer:${key}`)).toBe('reviewer:roles.reviewer')
  })

  it('keeps future CP hidden until real scoring is implemented', () => {
    expect(profileSource).not.toContain('cp_tenths')
  })
})
