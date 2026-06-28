import { createI18n } from 'vue-i18n'
import en from './locales/en.json'
import zhCN from './locales/zh-CN.json'

export type AppLocale = 'zh-CN' | 'en'

export const i18n = createI18n({
  legacy: false, // 使用 Composition API（useI18n）
  locale: 'zh-CN',
  fallbackLocale: 'en',
  messages: {
    'zh-CN': zhCN,
    en,
  },
})
