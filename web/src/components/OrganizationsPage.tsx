import { Link } from '@tanstack/react-router';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useState } from 'react';
import { Building2 } from 'lucide-react';

import { api } from '@/api/client';
import { normalizeLocale, translate } from '@/i18n';
import { useAuthStore } from '@/store/auth';
import { usePreferencesStore } from '@/store/preferences';

interface Organization {
  id: string;
  name: string;
  slug: string;
  createdBy?: string;
}

export function OrganizationsPage() {
  const queryClient = useQueryClient();
  const { data: orgs, isLoading, error } = useQuery({
    queryKey: ['organizations'],
    queryFn: () => api.get<{ items: Organization[] }>('/organizations'),
  });

  const user = useAuthStore((s) => s.user);
  const logout = useAuthStore((s) => s.logout);
  const uiLocale = usePreferencesStore((s) => s.uiLocale);
  const locale = normalizeLocale(user?.preferredLocale || uiLocale);
  const t = (key: string) => translate(locale, key);
  const [form, setForm] = useState({
    name: '',
    slug: '',
    description: '',
    visibility: 'public',
  });
  const [message, setMessage] = useState<string | null>(null);

  const createOrganization = useMutation({
    mutationFn: () =>
      api.post('/organizations', {
        name: form.name,
        slug: form.slug,
        description: form.description,
        visibility: form.visibility,
        createdBy: user?.id || '',
      }),
    onSuccess: () => {
      setForm({ name: '', slug: '', description: '', visibility: 'public' });
      setMessage(t('organizations.createSuccess'));
      void queryClient.invalidateQueries({ queryKey: ['organizations'] });
    },
  });

  return (
    <div className="min-h-screen bg-slate-50 flex flex-col p-4">
      <header className="flex justify-end mb-8 w-full max-w-4xl mx-auto">
        {user ? (
          <div className="flex items-center space-x-4">
            <span className="text-sm text-slate-600">{user.displayName}</span>
            <button onClick={() => logout()} className="text-sm text-slate-400 hover:text-red-500">{t('common.logout')}</button>
          </div>
        ) : (
          <Link to="/login" className="text-sm font-medium text-blue-600 hover:underline">{t('common.login')}</Link>
        )}
      </header>
      <div className="flex-1 flex items-center justify-center">
        <div className="w-full max-w-6xl grid gap-6 lg:grid-cols-[1.2fr_0.8fr]">
        <div className="bg-white p-8 rounded-lg shadow-sm border border-slate-200">
          <h1 className="text-2xl font-semibold text-slate-900 mb-6 flex items-center">
            <Building2 className="mr-2" />
            {t('organizations.title')}
          </h1>

          {isLoading && <div className="text-slate-500">{t('organizations.loading')}</div>}
          {error && <div className="text-red-500">{t('organizations.failed')}</div>}

          <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
            {orgs?.items?.map((org) => (
              <Link
                key={org.id}
                to="/$orgId/projects"
                params={{ orgId: org.id }}
                className="block p-6 border border-slate-200 rounded hover:border-blue-500 hover:shadow-sm transition-all"
              >
                <h2 className="text-lg font-medium text-slate-900">{org.name}</h2>
                <p className="text-sm text-slate-500 mt-1">{org.slug}</p>
              </Link>
            ))}
          </div>
          {orgs?.items?.length === 0 && (
            <div className="text-slate-500">{t('organizations.empty')}</div>
          )}
        </div>
        <div className="bg-white p-8 rounded-lg shadow-sm border border-slate-200">
          <h2 className="text-xl font-semibold text-slate-900">{t('organizations.createTitle')}</h2>
          <p className="mt-2 text-sm text-slate-500">{t('organizations.createSubtitle')}</p>

          <div className="mt-6 space-y-4">
            <div>
              <label className="block text-sm font-medium text-slate-700 mb-1">{t('organizations.name')}</label>
              <input value={form.name} onChange={(e) => setForm((prev) => ({ ...prev, name: e.target.value }))} className="w-full rounded border border-slate-300 px-3 py-2" />
            </div>
            <div>
              <label className="block text-sm font-medium text-slate-700 mb-1">{t('organizations.slug')}</label>
              <input value={form.slug} onChange={(e) => setForm((prev) => ({ ...prev, slug: e.target.value }))} className="w-full rounded border border-slate-300 px-3 py-2" />
            </div>
            <div>
              <label className="block text-sm font-medium text-slate-700 mb-1">{t('organizations.description')}</label>
              <textarea value={form.description} onChange={(e) => setForm((prev) => ({ ...prev, description: e.target.value }))} className="w-full rounded border border-slate-300 px-3 py-2 min-h-[100px]" />
            </div>
            <div>
              <label className="block text-sm font-medium text-slate-700 mb-1">{t('organizations.visibility')}</label>
              <select value={form.visibility} onChange={(e) => setForm((prev) => ({ ...prev, visibility: e.target.value }))} className="w-full rounded border border-slate-300 px-3 py-2">
                <option value="public">public</option>
                <option value="private">private</option>
              </select>
            </div>
            <button
              type="button"
              disabled={!user || createOrganization.isPending || !form.name || !form.slug}
              onClick={() => createOrganization.mutate()}
              className="w-full rounded bg-blue-600 px-4 py-3 text-white font-medium disabled:opacity-50"
            >
              {createOrganization.isPending ? t('common.saving') : t('organizations.createAction')}
            </button>
            {!user && <p className="text-sm text-slate-500">{t('common.login')} required.</p>}
            {message && <p className="text-sm text-green-600">{message}</p>}
          </div>
        </div>
        </div>
      </div>
    </div>
  );
}
