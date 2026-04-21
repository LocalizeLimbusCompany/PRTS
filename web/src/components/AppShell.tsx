import { Link } from '@tanstack/react-router';
import { Settings, Shield, LogOut, Building2 } from 'lucide-react';

import { useAuthStore } from '@/store/auth';
import { useTranslation } from '@/hooks/useTranslation';

export function AppShell({
  title,
  subtitle,
  actions,
  children,
}: {
  title: string;
  subtitle?: string;
  actions?: React.ReactNode;
  children: React.ReactNode;
}) {
  const user = useAuthStore((s) => s.user);
  const logout = useAuthStore((s) => s.logout);
  const { t } = useTranslation();

  return (
    <div className="min-h-screen bg-[radial-gradient(circle_at_top_left,_rgba(15,23,42,0.05),_transparent_35%),linear-gradient(180deg,#f8fafc_0%,#eef4ff_100%)]">
      <header className="border-b border-slate-200/80 bg-white/85 backdrop-blur">
        <div className="mx-auto flex max-w-7xl items-center justify-between px-6 py-4">
          <div className="flex items-center gap-3">
            <Link to="/organizations" className="flex h-11 w-11 items-center justify-center rounded-2xl bg-slate-900 text-white">
              <Building2 className="h-5 w-5" />
            </Link>
            <div>
              <div className="text-xs font-semibold uppercase tracking-[0.22em] text-slate-400">PRTS</div>
              <div className="text-lg font-semibold text-slate-900">{title}</div>
            </div>
          </div>

          <div className="flex items-center gap-3">
            {user?.platformRole === 'owner' || user?.platformRole === 'admin' ? (
              <Link to="/admin" className="inline-flex items-center gap-2 rounded-full border border-slate-200 bg-white px-4 py-2 text-sm font-medium text-slate-700 hover:border-slate-300">
                <Shield className="h-4 w-4" />
                {t('admin.title')}
              </Link>
            ) : null}
            {user ? (
              <>
                <Link to="/settings" className="inline-flex items-center gap-2 rounded-full border border-slate-200 bg-white px-4 py-2 text-sm font-medium text-slate-700 hover:border-slate-300">
                  <Settings className="h-4 w-4" />
                  {t('settings.title')}
                </Link>
                <button onClick={() => logout()} className="inline-flex items-center gap-2 rounded-full border border-red-100 bg-red-50 px-4 py-2 text-sm font-medium text-red-600 hover:bg-red-100">
                  <LogOut className="h-4 w-4" />
                  {t('common.logout')}
                </button>
              </>
            ) : (
              <Link to="/login" className="rounded-full bg-slate-900 px-4 py-2 text-sm font-semibold text-white">
                {t('common.login')}
              </Link>
            )}
          </div>
        </div>
      </header>

      <main className="mx-auto max-w-7xl px-6 py-10">
        <div className="flex flex-wrap items-end justify-between gap-4">
          <div>
            <div className="text-4xl font-semibold tracking-tight text-slate-950">{title}</div>
            {subtitle ? <p className="mt-3 max-w-3xl text-sm leading-7 text-slate-500">{subtitle}</p> : null}
          </div>
          {actions}
        </div>
        <div className="mt-8">{children}</div>
      </main>
    </div>
  );
}
