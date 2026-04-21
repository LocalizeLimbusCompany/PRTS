import { create } from 'zustand';
import { persist } from 'zustand/middleware';

export interface User {
  id: string;
  email?: string;
  username?: string;
  displayName: string;
  avatarUrl?: string;
  platformRole?: string;
  preferredLocale: string;
  preferredSourceLanguage: string;
  status?: string;
}

interface AuthState {
  user: User | null;
  token: string | null;
  setAuth: (user: User, token: string) => void;
  updateUser: (user: Partial<User>) => void;
  logout: () => void;
}

export const useAuthStore = create<AuthState>()(
  persist(
    (set) => ({
      user: null,
      token: null,
      setAuth: (user, token) => set({ user, token }),
      updateUser: (user) => set((state) => ({ user: state.user ? { ...state.user, ...user } : state.user })),
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
