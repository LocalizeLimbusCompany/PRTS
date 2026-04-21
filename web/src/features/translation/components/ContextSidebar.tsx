import { useQuery } from '@tanstack/react-query';
import { History, LoaderCircle } from 'lucide-react';

import { api } from '@/api/client';
import { useTranslation } from '@/hooks/useTranslation';

interface Props {
  projectId: string;
  unitId?: string | null;
}

interface RevisionItem {
  id: string;
  revisionNo: number;
  beforeTargetText: string;
  afterTargetText: string;
  beforeStatus: string;
  afterStatus: string;
  changeNote: string;
  changedAt: string;
  changedBy?: {
    id: string;
    name: string;
  };
}

export default function ContextSidebar({ projectId, unitId }: Props) {
  const { t } = useTranslation();
  const { data, isLoading } = useQuery({
    queryKey: ['unit-history', projectId, unitId],
    enabled: Boolean(unitId),
    queryFn: () => api.get<{ items: RevisionItem[] }>(`/projects/${projectId}/units/${unitId}/history`),
  });

  return (
    <div className="flex h-full flex-col">
      <div className="shrink-0 border-b border-slate-200 bg-white p-3">
        <h3 className="flex items-center text-sm font-semibold text-slate-800">
          <History className="mr-2 h-4 w-4" />
          {t('history.title')}
        </h3>
      </div>
      <div className="flex-1 overflow-y-auto p-4">
        {!unitId ? (
          <Empty label={t('history.empty')} />
        ) : isLoading ? (
          <div className="flex items-center justify-center py-10 text-sm text-slate-500">
            <LoaderCircle className="mr-2 h-4 w-4 animate-spin" />
            {t('common.loading')}
          </div>
        ) : !(data?.items?.length) ? (
          <Empty label={t('history.empty')} />
        ) : (
          <div className="space-y-3">
            {data.items.map((item) => (
              <article key={item.id} className="rounded-2xl border border-slate-200 bg-white p-4 shadow-sm">
                <div className="flex items-center justify-between gap-3">
                  <div className="text-xs font-semibold uppercase tracking-[0.18em] text-slate-400">#{item.revisionNo}</div>
                  <div className="text-[11px] text-slate-400">{new Date(item.changedAt).toLocaleString()}</div>
                </div>
                <div className="mt-3 grid gap-3">
                  <div className="rounded-xl bg-slate-50 p-3">
                    <div className="text-[11px] font-semibold uppercase tracking-[0.18em] text-slate-400">{t('history.before')}</div>
                    <div className="mt-2 whitespace-pre-wrap text-sm text-slate-700">{item.beforeTargetText || '∅'}</div>
                  </div>
                  <div className="rounded-xl bg-blue-50 p-3">
                    <div className="text-[11px] font-semibold uppercase tracking-[0.18em] text-blue-500">{t('history.after')}</div>
                    <div className="mt-2 whitespace-pre-wrap text-sm text-slate-700">{item.afterTargetText || '∅'}</div>
                  </div>
                </div>
                <div className="mt-3 text-xs text-slate-500">
                  {item.changedBy?.name || '-'} · {item.beforeStatus} → {item.afterStatus}
                </div>
                {item.changeNote ? <div className="mt-2 text-xs leading-6 text-slate-500">{item.changeNote}</div> : null}
              </article>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

function Empty({ label }: { label: string }) {
  return (
    <div className="mt-10 text-center text-sm text-slate-500">
      <History className="mx-auto mb-2 h-8 w-8 opacity-20" />
      <p>{label}</p>
    </div>
  );
}
