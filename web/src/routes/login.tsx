import { createFileRoute, useNavigate } from '@tanstack/react-router';
import React, { useState } from 'react';
import { api } from '@/api/client';
import { normalizeLocale, translate } from '@/i18n';
import { getErrorMessage } from '@/lib/getErrorMessage';
import { useAuthStore, type User } from '@/store/auth';
import { usePreferencesStore } from '@/store/preferences';

export const Route = createFileRoute('/login')({
  component: Login,
});

interface LoginResponse {
  token: string;
  user: User;
}

function Login() {
  const [email, setEmail] = useState('admin@example.com');
  const [password, setPassword] = useState('admin123');
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(false);
  const navigate = useNavigate();
  const setAuth = useAuthStore((s) => s.setAuth);
  const uiLocale = usePreferencesStore((s) => s.uiLocale);
  const setUiLocale = usePreferencesStore((s) => s.setUiLocale);
  const locale = normalizeLocale(uiLocale);
  const t = (key: string) => translate(locale, key);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError('');
    setLoading(true);

    try {
      const data = await api.post<LoginResponse>('/auth/login', { email, password });
      localStorage.setItem('prts_token', data.token);
      setAuth(data.user, data.token);
      setUiLocale(data.user.preferredLocale || locale);
      navigate({ to: '/organizations' });
    } catch (error: unknown) {
      setError(getErrorMessage(error));
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="min-h-screen bg-slate-50 flex items-center justify-center p-4">
      <div className="bg-white max-w-sm w-full p-8 rounded-lg shadow-sm border border-slate-200">
        <h1 className="text-2xl font-semibold text-slate-900 mb-6 text-center">PRTS Translation</h1>
        <h1 className="text-2xl font-semibold text-slate-900 mb-6 text-center">{t('loginPage.title')}</h1>
        
        {error && (
          <div className="bg-red-50 text-red-600 p-3 rounded mb-4 text-sm">
            {error}
          </div>
        )}

        <form onSubmit={handleSubmit} className="space-y-4">
          <div>
            <label className="block text-sm font-medium text-slate-700 mb-1">{t('loginPage.email')}</label>
            <input
              type="email"
              className="w-full px-3 py-2 border border-slate-300 rounded focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-blue-500"
              value={email}
              onChange={(e) => setEmail(e.target.value)}
              required
            />
          </div>
          <div>
            <label className="block text-sm font-medium text-slate-700 mb-1">{t('loginPage.password')}</label>
            <input
              type="password"
              className="w-full px-3 py-2 border border-slate-300 rounded focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-blue-500"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              required
            />
          </div>
          <button
            type="submit"
            disabled={loading}
            className="w-full bg-blue-600 text-white font-medium py-2 px-4 rounded hover:bg-blue-700 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:ring-offset-2 disabled:opacity-50"
          >
            {loading ? t('loginPage.loading') : t('loginPage.submit')}
          </button>
        </form>
      </div>
    </div>
  );
}
