export interface LanguageOption {
  code: string;
  label: string;
}

export const COMMON_LANGUAGE_OPTIONS: LanguageOption[] = [
  { code: 'zh-CN', label: '简体中文' },
  { code: 'en', label: 'English' },
  { code: 'ja', label: '日本语' },
  { code: 'ko', label: '한국어' },
  { code: 'fr', label: 'Français' },
  { code: 'de', label: 'Deutsch' },
  { code: 'es', label: 'Español' },
  { code: 'ru', label: 'Русский' },
];

export function languageLabel(code: string) {
  return COMMON_LANGUAGE_OPTIONS.find((item) => item.code === code)?.label ?? code;
}
