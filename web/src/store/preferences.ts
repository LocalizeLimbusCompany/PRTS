import { create } from 'zustand';
import { persist } from 'zustand/middleware';

interface PreferencesState {
  uiLocale: string;
  setUiLocale: (locale: string) => void;
}

export const usePreferencesStore = create<PreferencesState>()(
  persist(
    (set) => ({
      uiLocale: 'en-US',
      setUiLocale: (locale) => set({ uiLocale: locale }),
    }),
    {
      name: 'prts-preferences',
    }
  )
);