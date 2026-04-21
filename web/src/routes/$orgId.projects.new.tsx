import { createFileRoute, redirect, useNavigate } from '@tanstack/react-router';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useState } from 'react';

import { AppShell } from '@/components/AppShell';
import { getOrganization } from '@/api/organizations';
import { createProject } from '@/api/projects';
import { useTranslation } from '@/hooks/useTranslation';
import { COMMON_LANGUAGE_OPTIONS } from '@/lib/languages';

export const Route = createFileRoute('/$orgId/projects/new')({
  beforeLoad: () => {
    const authStorage = JSON.parse(localStorage.getItem('auth-storage') || '{}');
    if (!authStorage.state?.token) {
      throw redirect({ to: '/login' });
    }
  },
  component: CreateProjectPage,
});

function CreateProjectPage() {
  const { orgId } = Route.useParams();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const { t } = useTranslation();
  const { data: organization } = useQuery({
    queryKey: ['organization', orgId],
    queryFn: () => getOrganization(orgId),
  });

  const [form, setForm] = useState({
    name: '',
    slug: '',
    description: '',
    targetLanguage: 'zh-CN',
    sourceLanguages: ['en', 'ja'],
    visibility: 'public',
    guestPolicy: 'read',
  });

  const mutation = useMutation({
    mutationFn: () =>
      createProject(orgId, {
        ...form,
        sourceLanguages: form.sourceLanguages,
      }),
    onSuccess: async (item) => {
      await queryClient.invalidateQueries({ queryKey: ['projects', orgId] });
      void navigate({ to: '/project/$projectId/dashboard', params: { projectId: item.id } });
    },
  });

  return (
    <AppShell title={t('projects.createTitle')} subtitle={t('projects.createSubtitle')}>
      {!organization?.canCreateProject ? (
        <div className="rounded-[24px] border border-amber-200 bg-amber-50 px-3 py-2 text-sm text-amber-700">
          {t(`errors.${organization?.createRestrictedReason || 'organization_permission_required'}`)}
        </div>
      ) : (
        <div className="mx-auto max-w-4xl rounded-[30px] border border-slate-200 bg-white p-3 shadow-[0_30px_70px_-45px_rgba(15,23,42,0.35)]">
          <div className="grid gap-2">
            <label className="grid gap-2 text-sm font-medium text-slate-700">
              <span>{t('projects.name')}</span>
              <input value={form.name} onChange={(e) => setForm((prev) => ({ ...prev, name: e.target.value }))} className="rounded-sm border border-slate-300 px-2 py-1.5 outline-none focus:border-slate-900" />
            </label>
            <label className="grid gap-2 text-sm font-medium text-slate-700">
              <span>{t('projects.slug')}</span>
              <input value={form.slug} onChange={(e) => setForm((prev) => ({ ...prev, slug: e.target.value }))} className="rounded-sm border border-slate-300 px-2 py-1.5 outline-none focus:border-slate-900" />
            </label>
            <label className="grid gap-2 text-sm font-medium text-slate-700">
              <span>{t('projects.description')}</span>
              <textarea value={form.description} onChange={(e) => setForm((prev) => ({ ...prev, description: e.target.value }))} className="min-h-[120px] rounded-sm border border-slate-300 px-2 py-1.5 outline-none focus:border-slate-900" />
            </label>
            <div className="grid gap-2 md:grid-cols-2">
              <label className="grid gap-2 text-sm font-medium text-slate-700">
                <span>{t('projects.targetLanguage')}</span>
                <select value={form.targetLanguage} onChange={(e) => setForm((prev) => ({ ...prev, targetLanguage: e.target.value }))} className="rounded-sm border border-slate-300 px-2 py-1.5 outline-none focus:border-slate-900">
                  {COMMON_LANGUAGE_OPTIONS.map((option) => <option key={option.code} value={option.code}>{option.label}</option>)}
                </select>
              </label>
              <label className="grid gap-2 text-sm font-medium text-slate-700">
                <span>{t('projects.sourceLanguages')}</span>
                <select
                  multiple
                  value={form.sourceLanguages}
                  onChange={(e) => setForm((prev) => ({
                    ...prev,
                    sourceLanguages: Array.from(e.target.selectedOptions).map((item) => item.value),
                  }))}
                  className="min-h-[80px] rounded-sm border border-slate-300 px-2 py-1.5 outline-none focus:border-slate-900"
                >
                  {COMMON_LANGUAGE_OPTIONS.map((option) => <option key={option.code} value={option.code}>{option.label}</option>)}
                </select>
              </label>
            </div>
            <div className="grid gap-2 md:grid-cols-2">
              <label className="grid gap-2 text-sm font-medium text-slate-700">
                <span>{t('projects.visibility')}</span>
                <select value={form.visibility} onChange={(e) => setForm((prev) => ({ ...prev, visibility: e.target.value }))} className="rounded-sm border border-slate-300 px-2 py-1.5 outline-none focus:border-slate-900">
                  <option value="public">public</option>
                  <option value="private">private</option>
                </select>
              </label>
              <label className="grid gap-2 text-sm font-medium text-slate-700">
                <span>{t('projects.guestPolicy')}</span>
                <select value={form.guestPolicy} onChange={(e) => setForm((prev) => ({ ...prev, guestPolicy: e.target.value }))} className="rounded-sm border border-slate-300 px-2 py-1.5 outline-none focus:border-slate-900">
                  <option value="read">read</option>
                  <option value="closed">closed</option>
                </select>
              </label>
            </div>
          </div>

          <div className="mt-3 flex items-center gap-2">
            <button
              type="button"
              onClick={() => mutation.mutate()}
              disabled={mutation.isPending || !form.name || !form.slug || !form.targetLanguage || form.sourceLanguages.length === 0}
              className="rounded-full bg-slate-900 px-3 py-3 text-sm font-semibold text-white disabled:opacity-50"
            >
              {mutation.isPending ? t('common.saving') : t('projects.createAction')}
            </button>
            {mutation.isError ? <span className="text-sm text-red-500">{t('projects.failed')}</span> : null}
          </div>
        </div>
      )}
    </AppShell>
  );
}
