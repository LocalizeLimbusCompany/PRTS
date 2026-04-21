import { createFileRoute, redirect } from '@tanstack/react-router';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';

import { AppShell } from '@/components/AppShell';
import { getPlatformOverview, getPlatformUsers, updatePlatformSettings, updatePlatformUser } from '@/api/admin';
import { useTranslation } from '@/hooks/useTranslation';
import { useAuthStore } from '@/store/auth';

export const Route = createFileRoute('/admin' as any)({
  beforeLoad: () => {
    const authStorage = JSON.parse(localStorage.getItem('auth-storage') || '{}');
    if (!authStorage.state?.token) {
      throw redirect({ to: '/login' });
    }
  },
  component: AdminPage,
});

function AdminPage() {
  const queryClient = useQueryClient();
  const { t } = useTranslation();
  const currentUser = useAuthStore((s) => s.user);

  const { data: overview } = useQuery({
    queryKey: ['admin-overview'],
    queryFn: getPlatformOverview,
  });

  const { data: usersData } = useQuery({
    queryKey: ['admin-users'],
    queryFn: getPlatformUsers,
  });

  const updateSettingsMutation = useMutation({
    mutationFn: updatePlatformSettings,
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['admin-overview'] }),
  });

  const updateUserMutation = useMutation({
    mutationFn: ({ userId, platformRole, status }: { userId: string; platformRole: string; status: string }) =>
      updatePlatformUser(userId, { platformRole, status }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['admin-users'] }),
  });

  if (currentUser?.platformRole !== 'owner' && currentUser?.platformRole !== 'admin') {
    return (
      <AppShell title={t('admin.title')} subtitle={t('admin.subtitle')}>
        <div className="rounded-[24px] border border-amber-200 bg-amber-50 px-6 py-10 text-sm text-amber-700">
          {t('admin.forbidden')}
        </div>
      </AppShell>
    );
  }

  return (
    <AppShell title={t('admin.title')} subtitle={t('admin.subtitle')}>
      <div className="grid gap-6">
        <section className="grid gap-4 md:grid-cols-3">
          <StatCard label={t('admin.userCount')} value={overview?.userCount ?? 0} />
          <StatCard label={t('admin.organizationCount')} value={overview?.organizationCount ?? 0} />
          <StatCard label={t('admin.projectCount')} value={overview?.projectCount ?? 0} />
        </section>

        <section className="rounded-[28px] border border-slate-200 bg-white p-6 shadow-[0_30px_60px_-40px_rgba(15,23,42,0.3)]">
          <div className="text-lg font-semibold text-slate-900">{t('admin.settingsTitle')}</div>
          <div className="mt-5 grid gap-4 md:grid-cols-2">
            <ToggleCard
              title={t('admin.allowCreateOrganization')}
              checked={overview?.settings.allowUserCreateOrganization ?? false}
              onChange={(checked) =>
                updateSettingsMutation.mutate({
                  allowUserCreateOrganization: checked,
                  allowUserCreateProject: overview?.settings.allowUserCreateProject ?? false,
                })
              }
            />
            <ToggleCard
              title={t('admin.allowCreateProject')}
              checked={overview?.settings.allowUserCreateProject ?? false}
              onChange={(checked) =>
                updateSettingsMutation.mutate({
                  allowUserCreateOrganization: overview?.settings.allowUserCreateOrganization ?? false,
                  allowUserCreateProject: checked,
                })
              }
            />
          </div>
        </section>

        <section className="rounded-[28px] border border-slate-200 bg-white p-6 shadow-[0_30px_60px_-40px_rgba(15,23,42,0.3)]">
          <div className="text-lg font-semibold text-slate-900">{t('admin.usersTitle')}</div>
          <div className="mt-5 space-y-3">
            {usersData?.items?.map((user) => (
              <div key={user.id} className="flex flex-wrap items-center justify-between gap-4 rounded-[22px] border border-slate-200 px-5 py-4">
                <div>
                  <div className="font-semibold text-slate-900">{user.displayName}</div>
                  <div className="text-sm text-slate-500">{user.email}</div>
                </div>
                <div className="flex flex-wrap items-center gap-3">
                  <select
                    value={user.platformRole}
                    disabled={currentUser.platformRole !== 'owner' && user.platformRole === 'owner'}
                    onChange={(e) =>
                      updateUserMutation.mutate({
                        userId: user.id,
                        platformRole: e.target.value,
                        status: user.status || 'active',
                      })
                    }
                    className="rounded-full border border-slate-300 px-4 py-2 text-sm"
                  >
                    <option value="owner">owner</option>
                    <option value="admin">admin</option>
                    <option value="user">user</option>
                  </select>
                  <select
                    value={user.status}
                    onChange={(e) =>
                      updateUserMutation.mutate({
                        userId: user.id,
                        platformRole: user.platformRole || 'user',
                        status: e.target.value,
                      })
                    }
                    className="rounded-full border border-slate-300 px-4 py-2 text-sm"
                  >
                    <option value="active">active</option>
                    <option value="disabled">disabled</option>
                  </select>
                </div>
              </div>
            ))}
          </div>
        </section>
      </div>
    </AppShell>
  );
}

function StatCard({ label, value }: { label: string; value: number }) {
  return (
    <div className="rounded-[24px] border border-slate-200 bg-white p-6 shadow-[0_30px_60px_-40px_rgba(15,23,42,0.25)]">
      <div className="text-sm font-medium text-slate-500">{label}</div>
      <div className="mt-2 text-4xl font-semibold tracking-tight text-slate-950">{value.toLocaleString()}</div>
    </div>
  );
}

function ToggleCard({ title, checked, onChange }: { title: string; checked: boolean; onChange: (checked: boolean) => void }) {
  return (
    <button type="button" onClick={() => onChange(!checked)} className={`rounded-[24px] border px-5 py-5 text-left transition ${checked ? 'border-emerald-300 bg-emerald-50' : 'border-slate-200 bg-slate-50'}`}>
      <div className="flex items-center justify-between gap-4">
        <div className="text-sm font-semibold text-slate-900">{title}</div>
        <div className={`h-7 w-12 rounded-full p-1 transition ${checked ? 'bg-emerald-500' : 'bg-slate-300'}`}>
          <div className={`h-5 w-5 rounded-full bg-white transition ${checked ? 'translate-x-5' : ''}`} />
        </div>
      </div>
    </button>
  );
}
