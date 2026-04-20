import { create } from 'zustand';
import { persist } from 'zustand/middleware';

import { normalizeLocale, type Locale } from '@/i18n';

interface PreferencesState {
  uiLocale: Locale;
  setUiLocale: (locale: string) => void;
}

export const usePreferencesStore = create<PreferencesState>()(
  persist(
    (set) => ({
      uiLocale: 'en-US',
      setUiLocale: (locale) => set({ uiLocale: normalizeLocale(locale) }),
    }),
    {
      name: 'prts-preferences',
    }
  )
);
