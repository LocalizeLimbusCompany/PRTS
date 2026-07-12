import { createI18n } from 'vue-i18n'
import en from './locales/en.json'
import zhCN from './locales/zh-CN.json'

export type AppLocale = 'zh-CN' | 'en'

const STORAGE_KEY = 'prts_locale'

function initialLocale(): AppLocale {
  const stored = localStorage.getItem(STORAGE_KEY)
  if (stored === 'zh-CN' || stored === 'en') return stored
  const browserLocale = navigator.language.toLowerCase()
  if (browserLocale.startsWith('zh')) return 'zh-CN'
  if (browserLocale.startsWith('en')) return 'en'
  return 'zh-CN'
}

export function setLocale(locale: AppLocale): void {
  i18n.global.locale.value = locale
  localStorage.setItem(STORAGE_KEY, locale)
  document.documentElement.lang = locale
}

export const i18n = createI18n({
  legacy: false, // 使用 Composition API（useI18n）
  locale: initialLocale(),
  fallbackLocale: 'en',
  messages: {
    'zh-CN': zhCN,
    en,
  },
})

document.documentElement.lang = i18n.global.locale.value
