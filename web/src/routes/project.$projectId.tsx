import { createFileRoute, Outlet, Link } from '@tanstack/react-router';
import { useQuery } from '@tanstack/react-query';
import { normalizeLocale, translate } from '@/i18n';
import { api } from '@/api/client';
import { useAuthStore } from '@/store/auth';
import { usePreferencesStore } from '@/store/preferences';
import { Settings, LogOut, FileText, Activity, LayoutDashboard, Globe, Shield, Building2, FolderKanban } from 'lucide-react';

export const Route = createFileRoute('/project/$projectId')({
  component: ProjectLayout,
});

function ProjectLayout() {
  const { projectId } = Route.useParams();
  const user = useAuthStore((s) => s.user);
  const logout = useAuthStore((s) => s.logout);
  const uiLocale = usePreferencesStore((s) => s.uiLocale);
  const locale = normalizeLocale(user?.preferredLocale || uiLocale);
  const t = (key: string) => translate(locale, key);
  const { data: project } = useQuery({
    queryKey: ['project-layout', projectId],
    queryFn: () => api.get<{ id: string; name: string; organizationId: string }>(`/projects/${projectId}`),
  });

  return (
    <div className="h-screen flex flex-col bg-slate-50 text-slate-900 overflow-hidden">
      {/* Top Navbar */}
      <header className="h-8 bg-white border-b border-slate-200 flex items-center px-2 justify-between shrink-0">
        <div className="flex items-center space-x-6">
          <Link to="/organizations" className="font-bold text-slate-800 tracking-tight">
            PRTS
          </Link>

          <div className="flex items-center gap-2 text-xs text-slate-400">
            <Link to="/organizations" className="inline-flex items-center gap-1.5 rounded-full px-2.5 py-1 hover:bg-slate-100 hover:text-slate-700">
              <Building2 size={13} />
              <span>{t('organizations.title')}</span>
            </Link>
            {project?.organizationId ? (
              <Link to="/$orgId/projects" params={{ orgId: project.organizationId }} className="inline-flex items-center gap-1.5 rounded-full px-2.5 py-1 hover:bg-slate-100 hover:text-slate-700">
                <FolderKanban size={13} />
                <span>{t('projects.title')}</span>
              </Link>
            ) : null}
          </div>

          <nav className="flex space-x-1 overflow-x-auto shrink-0 pb-1 -mb-1 scrollbar-hide">
            <NavLink to={`/project/${projectId}/dashboard`} icon={<LayoutDashboard size={14} />} label="Overview" />
            <NavLink to={`/project/${projectId}/workbench`} icon={<Activity size={14} />} label="Workbench" />  
            <NavLink to={`/project/${projectId}/documents`} icon={<FileText size={14} />} label="Documents" />  
            <NavLink to={`/project/${projectId}/jobs`} icon={<FileText size={14} />} label="Jobs" />
            <NavLink to={`/project/${projectId}/history`} icon={<FileText size={14} />} label="History" />
            <NavLink to={`/project/${projectId}/members`} icon={<FileText size={14} />} label="Members" />
            <NavLink to={`/project/${projectId}/settings`} icon={<Settings size={14} />} label="Settings" />    
          </nav>
        </div>

        <div className="flex items-center space-x-4">
          {(user?.platformRole === 'owner' || user?.platformRole === 'admin') && (
            <Link to="/admin" className="text-sm font-medium text-slate-600 hover:text-slate-900 flex items-center">
              <Shield size={14} className="mr-1.5" />
              {t('admin.title')}
            </Link>
          )}
          {user ? (
            <>
              <div className="text-sm font-medium text-slate-600 flex items-center">
                <Globe size={14} className="mr-1.5" />
                {user.displayName} ({user.preferredLocale})
              </div>
              <button
                onClick={() => logout()}
                className="text-slate-400 hover:text-red-500 transition-colors"
                title={t('common.logout')}
              >
                <LogOut size={16} />
              </button>
            </>
          ) : (
            <Link to="/login" className="text-sm font-medium text-blue-600 hover:underline">
              {t('common.login')}
            </Link>
          )}
        </div>
      </header>

      {/* Main Content Area */}
      <main className="flex-1 overflow-hidden">
        <Outlet />
      </main>
    </div>
  );
}

function NavLink({ to, icon, label }: { to: string; icon: React.ReactNode; label: string }) {
  return (
    <Link
      to={to as any}
      className="flex items-center space-x-1.5 px-3 py-1.5 text-sm rounded transition-colors"
      activeProps={{
        className: 'bg-slate-100 text-blue-600 font-medium'
      }}
      inactiveProps={{
        className: 'text-slate-600 hover:bg-slate-50 hover:text-slate-900'
      }}
    >
      {icon}
      <span>{label}</span>
    </Link>
  );
}
