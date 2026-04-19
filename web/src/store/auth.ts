import { create } from 'zustand';
import { persist } from 'zustand/middleware';

export interface User {
  id: string;
  displayName: string;
  preferredLocale: string;
  preferredSourceLanguage: string;
}

interface AuthState {
  user: User | null;
  token: string | null;
  setAuth: (user: User, token: string) => void;
  logout: () => void;
}

export const useAuthStore = create<AuthState>()(
  persist(
    (set) => ({
      user: null,
      token: null,
      setAuth: (user, token) => set({ user, token }),
      logout: () => {
        localStorage.removeItem('prts_token');
        set({ user: null, token: null });
      },
    }),
    {
      name: 'auth-storage',
    }
  )
);