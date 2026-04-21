import { createFileRoute, redirect, useNavigate } from '@tanstack/react-router';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useState } from 'react';

import { AppShell } from '@/components/AppShell';
import { createOrganization } from '@/api/organizations';
import { useTranslation } from '@/hooks/useTranslation';
import { useAuthStore } from '@/store/auth';

export const Route = createFileRoute('/organizations/new')({
  beforeLoad: () => {
    const authStorage = JSON.parse(localStorage.getItem('auth-storage') || '{}');
    if (!authStorage.state?.token) {
      throw redirect({ to: '/login' });
    }
  },
  component: OrganizationCreatePage,
});

function OrganizationCreatePage() {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const { t } = useTranslation();
  const user = useAuthStore((s) => s.user);
  const [form, setForm] = useState({ name: '', slug: '', description: '', visibility: 'public' });

  const mutation = useMutation({
    mutationFn: () => createOrganization(form),
    onSuccess: async (item) => {
      await queryClient.invalidateQueries({ queryKey: ['organizations'] });
      void navigate({ to: '/$orgId/projects', params: { orgId: item.id } });
    },
  });

  if (!user || (user.platformRole !== 'owner' && user.platformRole !== 'admin')) {
    return (
      <AppShell title={t('organizations.createTitle')} subtitle={t('organizations.createSubtitle')}>
        <div className="rounded-[24px] border border-amber-200 bg-amber-50 px-3 py-2 text-sm text-amber-700">
          {t('organizations.createRestrictedHint')}
        </div>
      </AppShell>
    );
  }

  return (
    <AppShell title={t('organizations.createTitle')} subtitle={t('organizations.createSubtitle')}>
      <div className="mx-auto max-w-3xl rounded-[30px] border border-slate-200 bg-white p-3 shadow-[0_30px_70px_-45px_rgba(15,23,42,0.35)]">
        <div className="grid gap-2">
          <label className="grid gap-2 text-sm font-medium text-slate-700">
            <span>{t('organizations.name')}</span>
            <input value={form.name} onChange={(e) => setForm((prev) => ({ ...prev, name: e.target.value }))} className="rounded-sm border border-slate-300 px-2 py-1.5 outline-none focus:border-slate-900" />
          </label>
          <label className="grid gap-2 text-sm font-medium text-slate-700">
            <span>{t('organizations.slug')}</span>
            <input value={form.slug} onChange={(e) => setForm((prev) => ({ ...prev, slug: e.target.value }))} className="rounded-sm border border-slate-300 px-2 py-1.5 outline-none focus:border-slate-900" />
          </label>
          <label className="grid gap-2 text-sm font-medium text-slate-700">
            <span>{t('organizations.description')}</span>
            <textarea value={form.description} onChange={(e) => setForm((prev) => ({ ...prev, description: e.target.value }))} className="min-h-[140px] rounded-sm border border-slate-300 px-2 py-1.5 outline-none focus:border-slate-900" />
          </label>
          <label className="grid gap-2 text-sm font-medium text-slate-700">
            <span>{t('organizations.visibility')}</span>
            <select value={form.visibility} onChange={(e) => setForm((prev) => ({ ...prev, visibility: e.target.value }))} className="rounded-sm border border-slate-300 px-2 py-1.5 outline-none focus:border-slate-900">
              <option value="public">public</option>
              <option value="private">private</option>
            </select>
          </label>
        </div>

        <div className="mt-3 flex items-center gap-2">
          <button
            type="button"
            onClick={() => mutation.mutate()}
            disabled={mutation.isPending || !form.name || !form.slug}
            className="rounded-full bg-slate-900 px-3 py-3 text-sm font-semibold text-white disabled:opacity-50"
          >
            {mutation.isPending ? t('common.saving') : t('organizations.createAction')}
          </button>
          {mutation.isError ? <span className="text-sm text-red-500">{t('organizations.failed')}</span> : null}
        </div>
      </div>
    </AppShell>
  );
}
