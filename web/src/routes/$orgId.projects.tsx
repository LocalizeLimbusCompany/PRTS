import { createFileRoute, Link } from '@tanstack/react-router';
import { useQuery } from '@tanstack/react-query';
import { ArrowRight, FolderKanban, Globe2, Lock, Plus } from 'lucide-react';

import { AppShell } from '@/components/AppShell';
import { getOrganization } from '@/api/organizations';
import { getProjectsByOrganization } from '@/api/projects';
import { useTranslation } from '@/hooks/useTranslation';
import { languageLabel } from '@/lib/languages';

export const Route = createFileRoute('/$orgId/projects')({
  component: ProjectsPage,
});

function ProjectsPage() {
  const { orgId } = Route.useParams();
  const { t } = useTranslation();

  const { data: org } = useQuery({
    queryKey: ['organization', orgId],
    queryFn: () => getOrganization(orgId),
  });

  const { data: projectsData, isLoading, error } = useQuery({
    queryKey: ['projects', orgId],
    queryFn: () => getProjectsByOrganization(orgId),
  });

  const projects = projectsData?.items ?? [];

  return (
    <AppShell
      title={org?.name || t('projects.title')}
      subtitle={org?.description || t('projects.subtitle')}
      actions={
        org?.canCreateProject ? (
          <Link to="/$orgId/projects/new" params={{ orgId }} className="inline-flex items-center gap-2 rounded-full bg-slate-900 px-3 py-3 text-sm font-semibold text-white shadow-sm shadow-slate-900/20">
            <Plus className="h-4 w-4" />
            {t('projects.createAction')}
          </Link>
        ) : null
      }
    >
      <div className="grid gap-1 lg:grid-cols-[1.18fr_0.82fr]">
        <section className="rounded-[28px] border border-slate-200 bg-white p-1.5 shadow-[0_30px_60px_-40px_rgba(15,23,42,0.3)]">
          {isLoading ? <div className="text-sm text-slate-500">{t('projects.loading')}</div> : null}
          {error ? <div className="text-sm text-red-500">{t('projects.failed')}</div> : null}

          <div className="grid gap-2 md:grid-cols-2">
            {projects.map((project) => (
              <Link
                key={project.id}
                to="/project/$projectId/dashboard"
                params={{ projectId: project.id }}
                className="group rounded-[24px] border border-slate-200 bg-[linear-gradient(180deg,#ffffff_0%,#f8fafc_100%)] p-2 transition hover:-translate-y-1 hover:border-slate-900 hover:shadow-sm"
              >
                <div className="flex items-start justify-between gap-2">
                  <div>
                    <div className="text-lg font-semibold text-slate-900">{project.name}</div>
                    <div className="mt-1 text-xs font-semibold uppercase tracking-[0.18em] text-slate-400">{project.slug}</div>
                  </div>
                  <ArrowRight className="h-5 w-5 text-slate-300 transition group-hover:text-slate-900" />
                </div>

                <div className="mt-2 flex items-center gap-1 text-sm text-slate-500">
                  {project.visibility === 'public' ? <Globe2 className="h-4 w-4" /> : <Lock className="h-4 w-4" />}
                  <span>{project.visibility}</span>
                </div>

                <div className="mt-5 rounded-sm bg-slate-950 px-2 py-2 text-sm text-white">
                  <div className="text-[11px] uppercase tracking-[0.2em] text-slate-400">{t('projects.languageLine')}</div>
                  <div className="mt-2 flex flex-wrap items-center gap-2">
                    <span>{project.sourceLanguages.map(languageLabel).join(' / ')}</span>
                    <span className="text-slate-500">→</span>
                    <span className="font-semibold text-emerald-300">{languageLabel(project.targetLanguage)}</span>
                  </div>
                </div>
              </Link>
            ))}
          </div>

          {!isLoading && projects.length === 0 ? (
            <div className="rounded-[24px] border border-dashed border-slate-300 bg-slate-50 px-3 py-16 text-center text-sm text-slate-500">
              {t('projects.empty')}
            </div>
          ) : null}
        </section>

        <aside className="rounded-[28px] border border-slate-200 bg-slate-900 p-1.5 text-white shadow-[0_30px_70px_-45px_rgba(15,23,42,0.7)]">
          <div className="inline-flex items-center gap-2 rounded-full border border-white/10 bg-white/5 px-3 py-1 text-xs font-semibold uppercase tracking-[0.2em] text-slate-200">
            <FolderKanban className="h-3.5 w-3.5" />
            {t('projects.workspace')}
          </div>
          <div className="mt-5 text-2xl font-semibold tracking-tight">{t('projects.sideTitle')}</div>
          <p className="mt-2 text-sm leading-tight text-slate-300">{t('projects.sideBody')}</p>

          <div className="mt-3 rounded-[24px] border border-white/10 bg-white/5 p-2">
            <div className="text-sm font-semibold text-white">{org?.canCreateProject ? t('projects.createEnabled') : t('projects.createRestricted')}</div>
            <p className="mt-3 text-sm leading-tight text-slate-300">{org?.canCreateProject ? t('projects.createEnabledHint') : t(`errors.${org?.createRestrictedReason || 'organization_permission_required'}`)}</p>
          </div>
        </aside>
      </div>
    </AppShell>
  );
}
