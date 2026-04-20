import { useAuthStore } from '../store/auth';
import { usePreferencesStore } from '../store/preferences';
import { translate } from '../i18n';

export function useTranslation() {
  const preferredLocale = useAuthStore((s) => s.user?.preferredLocale);
  const uiLocale = usePreferencesStore((s) => s.uiLocale);
  
  const currentLocale = preferredLocale || uiLocale || 'en-US';

  const t = (key: string) => {
    return translate(currentLocale, key);
  };

  return { t, locale: currentLocale };
}
