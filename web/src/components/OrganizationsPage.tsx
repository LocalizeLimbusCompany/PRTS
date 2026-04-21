import { Link } from '@tanstack/react-router';
import { useQuery } from '@tanstack/react-query';
import { ArrowRight, Lock, Plus, Sparkles } from 'lucide-react';

import { AppShell } from '@/components/AppShell';
import { getOrganizations } from '@/api/organizations';
import { useAuthStore } from '@/store/auth';
import { useTranslation } from '@/hooks/useTranslation';

export function OrganizationsPage() {
  const { t } = useTranslation();
  const user = useAuthStore((s) => s.user);
  const { data, isLoading, error } = useQuery({
    queryKey: ['organizations'],
    queryFn: getOrganizations,
  });

  const organizations = data?.items ?? [];
  const canCreateOrganization = Boolean(user && (user.platformRole === 'owner' || user.platformRole === 'admin'));

  return (
    <AppShell
      title={t('organizations.title')}
      subtitle={t('organizations.subtitle')}
      actions={
        canCreateOrganization ? (
          <Link to={"/organizations/new" as any} className="inline-flex items-center gap-2 rounded-full bg-slate-900 px-5 py-3 text-sm font-semibold text-white shadow-sm shadow-slate-900/20">
            <Plus className="h-4 w-4" />
            {t('organizations.createAction')}
          </Link>
        ) : null
      }
    >
      <div className="grid gap-6 lg:grid-cols-[1.15fr_0.85fr]">
        <section className="rounded-[28px] border border-slate-200 bg-white/95 p-6 shadow-[0_30px_60px_-40px_rgba(15,23,42,0.3)]">
          {isLoading ? <div className="text-sm text-slate-500">{t('organizations.loading')}</div> : null}
          {error ? <div className="text-sm text-red-500">{t('organizations.failed')}</div> : null}

          <div className="grid gap-4 md:grid-cols-2">
            {organizations.map((org) => (
              <Link
                key={org.id}
                to="/$orgId/projects"
                params={{ orgId: org.id }}
                className="group rounded-[24px] border border-slate-200 bg-[linear-gradient(180deg,#ffffff_0%,#f8fafc_100%)] p-5 transition hover:-translate-y-1 hover:border-slate-900 hover:shadow-xl"
              >
                <div className="flex items-start justify-between gap-4">
                  <div>
                    <div className="text-lg font-semibold text-slate-900">{org.name}</div>
                    <div className="mt-1 text-xs font-semibold uppercase tracking-[0.18em] text-slate-400">{org.slug}</div>
                  </div>
                  <ArrowRight className="h-5 w-5 text-slate-300 transition group-hover:text-slate-900" />
                </div>
                <p className="mt-4 line-clamp-3 text-sm leading-6 text-slate-500">{org.description || t('organizations.emptyDesc')}</p>
                <div className="mt-5 flex items-center justify-between text-xs text-slate-400">
                  <span>{org.visibility}</span>
                  <span>{org.canCreateProject ? t('organizations.projectCreateOpen') : t('organizations.projectCreateRestricted')}</span>
                </div>
              </Link>
            ))}
          </div>

          {!isLoading && organizations.length === 0 ? (
            <div className="rounded-[24px] border border-dashed border-slate-300 bg-slate-50 px-6 py-16 text-center text-sm text-slate-500">
              {t('organizations.empty')}
            </div>
          ) : null}
        </section>

        <aside className="rounded-[28px] border border-slate-200 bg-slate-950 p-6 text-white shadow-[0_30px_70px_-45px_rgba(15,23,42,0.7)]">
          <div className="inline-flex items-center gap-2 rounded-full border border-white/10 bg-white/5 px-3 py-1 text-xs font-semibold uppercase tracking-[0.2em] text-slate-200">
            <Sparkles className="h-3.5 w-3.5" />
            {t('organizations.governance')}
          </div>
          <div className="mt-5 text-2xl font-semibold tracking-tight">{t('organizations.sideTitle')}</div>
          <p className="mt-4 text-sm leading-7 text-slate-300">{t('organizations.sideBody')}</p>

          {!canCreateOrganization ? (
            <div className="mt-8 rounded-[24px] border border-amber-400/20 bg-amber-400/10 p-5">
              <div className="flex items-center gap-2 text-sm font-semibold text-amber-200">
                <Lock className="h-4 w-4" />
                {t('organizations.createRestricted')}
              </div>
              <p className="mt-3 text-sm leading-6 text-amber-100/85">{t('organizations.createRestrictedHint')}</p>
            </div>
          ) : (
            <div className="mt-8 rounded-[24px] border border-emerald-400/20 bg-emerald-400/10 p-5">
              <div className="text-sm font-semibold text-emerald-200">{t('organizations.createEnabled')}</div>
              <p className="mt-3 text-sm leading-6 text-emerald-100/85">{t('organizations.createEnabledHint')}</p>
            </div>
          )}
        </aside>
      </div>
    </AppShell>
  );
}
