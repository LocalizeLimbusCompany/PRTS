import { createFileRoute, Link } from '@tanstack/react-router';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useState } from 'react';
import { api } from '@/api/client';
import { normalizeLocale, translate } from '@/i18n';
import { FolderKanban, Globe, Lock } from 'lucide-react';

import { useAuthStore } from '@/store/auth';
import { usePreferencesStore } from '@/store/preferences';

export const Route = createFileRoute('/$orgId/projects')({
  component: Projects,
});

interface Project {
  id: string;
  name: string;
  slug: string;
  sourceLanguages: string[];
  targetLanguage: string;
  visibility: 'public' | 'private';
}

interface Organization {
  id: string;
  name: string;
  slug: string;
  createdBy?: string;
}

function Projects() {
  const { orgId } = Route.useParams();
  const queryClient = useQueryClient();
  
  const { data: projects, isLoading, error } = useQuery({
    queryKey: ['projects', orgId],
    queryFn: () => api.get<{ items: Project[] }>(`/organizations/${orgId}/projects`),
  });

  const user = useAuthStore(s => s.user);
  const logout = useAuthStore(s => s.logout);
  const uiLocale = usePreferencesStore((s) => s.uiLocale);
  const locale = normalizeLocale(user?.preferredLocale || uiLocale);
  const t = (key: string) => translate(locale, key);
  const [message, setMessage] = useState<string | null>(null);
  const [form, setForm] = useState({
    name: '',
    slug: '',
    description: '',
    targetLanguage: 'cn',
    sourceLanguages: 'en,jp,kr',
    visibility: 'public',
    guestPolicy: 'read',
  });

  const { data: organization } = useQuery({
    queryKey: ['organization', orgId],
    queryFn: () => api.get<Organization>(`/organizations/${orgId}`),
  });

  const canCreateProject = Boolean(user && organization?.createdBy === user.id);

  const createProject = useMutation({
    mutationFn: () =>
      api.post(`/organizations/${orgId}/projects`, {
        name: form.name,
        slug: form.slug,
        description: form.description,
        targetLanguage: form.targetLanguage,
        sourceLanguages: form.sourceLanguages.split(',').map((item) => item.trim()).filter(Boolean),
        visibility: form.visibility,
        guestPolicy: form.guestPolicy,
        createdBy: user?.id || '',
      }),
    onSuccess: () => {
      setForm({
        name: '',
        slug: '',
        description: '',
        targetLanguage: 'cn',
        sourceLanguages: 'en,jp,kr',
        visibility: 'public',
        guestPolicy: 'read',
      });
      setMessage(t('projects.createSuccess'));
      void queryClient.invalidateQueries({ queryKey: ['projects', orgId] });
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
      <div className="w-full max-w-7xl grid gap-6 lg:grid-cols-[1.15fr_0.85fr]">
      <div className="bg-white p-8 rounded-lg shadow-sm border border-slate-200">
        <h1 className="text-2xl font-semibold text-slate-900 mb-6 flex items-center">
          <FolderKanban className="mr-2" />
          {t('projects.title')}
        </h1>
        
        {isLoading && <div className="text-slate-500">{t('projects.loading')}</div>}
        {error && <div className="text-red-500">{t('projects.failed')}</div>}
        
        <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
          {projects?.items?.map((proj) => (
            <Link
              key={proj.id}
              to="/project/$projectId/workbench"
              params={{ projectId: proj.id }}
              className="block p-6 border border-slate-200 rounded hover:border-blue-500 hover:shadow-sm transition-all"
            >
              <div className="flex justify-between items-start mb-2">
                <h2 className="text-lg font-medium text-slate-900">{proj.name}</h2>
                {proj.visibility === 'public' ? (
                  <Globe className="w-4 h-4 text-slate-400" />
                ) : (
                  <Lock className="w-4 h-4 text-slate-400" />
                )}
              </div>
              <p className="text-sm text-slate-500 mb-4">{proj.slug}</p>
              
              <div className="flex items-center text-xs font-mono text-slate-600 bg-slate-50 p-2 rounded">
                <span className="truncate max-w-[120px]">{proj.sourceLanguages.join(', ')}</span>
                <span className="mx-2 text-slate-400">→</span>
                <span className="font-semibold text-blue-600">{proj.targetLanguage}</span>
              </div>
            </Link>
          ))}
        </div>
        {projects?.items?.length === 0 && (
          <div className="text-slate-500">{t('projects.empty')}</div>
        )}
        
        <div className="mt-8 pt-6 border-t border-slate-100">
          <Link to="/organizations" className="text-sm text-blue-600 hover:underline">
            {t('projects.back')}
          </Link>
        </div>
      </div>
      <div className="bg-white p-8 rounded-lg shadow-sm border border-slate-200">
        <h2 className="text-xl font-semibold text-slate-900">{t('projects.createTitle')}</h2>
        <p className="mt-2 text-sm text-slate-500">{t('projects.createSubtitle')}</p>

        <div className="mt-6 space-y-4">
          <div>
            <label className="block text-sm font-medium text-slate-700 mb-1">{t('projects.name')}</label>
            <input value={form.name} onChange={(e) => setForm((prev) => ({ ...prev, name: e.target.value }))} className="w-full rounded border border-slate-300 px-3 py-2" />
          </div>
          <div>
            <label className="block text-sm font-medium text-slate-700 mb-1">{t('projects.slug')}</label>
            <input value={form.slug} onChange={(e) => setForm((prev) => ({ ...prev, slug: e.target.value }))} className="w-full rounded border border-slate-300 px-3 py-2" />
          </div>
          <div>
            <label className="block text-sm font-medium text-slate-700 mb-1">{t('projects.description')}</label>
            <textarea value={form.description} onChange={(e) => setForm((prev) => ({ ...prev, description: e.target.value }))} className="w-full rounded border border-slate-300 px-3 py-2 min-h-[96px]" />
          </div>
          <div className="grid grid-cols-2 gap-4">
            <div>
              <label className="block text-sm font-medium text-slate-700 mb-1">{t('projects.targetLanguage')}</label>
              <input value={form.targetLanguage} onChange={(e) => setForm((prev) => ({ ...prev, targetLanguage: e.target.value }))} className="w-full rounded border border-slate-300 px-3 py-2" />
            </div>
            <div>
              <label className="block text-sm font-medium text-slate-700 mb-1">{t('projects.sourceLanguages')}</label>
              <input value={form.sourceLanguages} onChange={(e) => setForm((prev) => ({ ...prev, sourceLanguages: e.target.value }))} className="w-full rounded border border-slate-300 px-3 py-2" />
            </div>
          </div>
          <div className="grid grid-cols-2 gap-4">
            <div>
              <label className="block text-sm font-medium text-slate-700 mb-1">{t('projects.visibility')}</label>
              <select value={form.visibility} onChange={(e) => setForm((prev) => ({ ...prev, visibility: e.target.value }))} className="w-full rounded border border-slate-300 px-3 py-2">
                <option value="public">public</option>
                <option value="private">private</option>
              </select>
            </div>
            <div>
              <label className="block text-sm font-medium text-slate-700 mb-1">{t('projects.guestPolicy')}</label>
              <select value={form.guestPolicy} onChange={(e) => setForm((prev) => ({ ...prev, guestPolicy: e.target.value }))} className="w-full rounded border border-slate-300 px-3 py-2">
                <option value="read">read</option>
                <option value="closed">closed</option>
              </select>
            </div>
          </div>
          <button
            type="button"
            disabled={!canCreateProject || createProject.isPending || !form.name || !form.slug || !form.targetLanguage || !form.sourceLanguages}
            onClick={() => createProject.mutate()}
            className="w-full rounded bg-blue-600 px-4 py-3 text-white font-medium disabled:opacity-50"
          >
            {createProject.isPending ? t('common.saving') : t('projects.createAction')}
          </button>
          {!canCreateProject && <p className="text-sm text-slate-500">{t('projects.createSubtitle')}</p>}
          {message && <p className="text-sm text-green-600">{message}</p>}
        </div>
      </div>
      </div>
      </div>
    </div>
  );
}
