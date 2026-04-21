import { createFileRoute } from '@tanstack/react-router';
import { useQuery } from '@tanstack/react-query';
import { useMemo, useState } from 'react';

import { getProjectHistory } from '@/api/projects';
import { RevisionChange } from '@/components/RevisionChange';
import { useTranslation } from '@/hooks/useTranslation';

export const Route = createFileRoute('/project/$projectId/history')({
  component: HistoryPage,
});

function HistoryPage() {
  const { projectId } = Route.useParams();
  const { t } = useTranslation();
  const [documentId, setDocumentId] = useState('');
  const [key, setKey] = useState('');
  const [status, setStatus] = useState('');

  const params = useMemo(() => {
    const search = new URLSearchParams();
    if (documentId) search.set('documentId', documentId);
    if (key) search.set('key', key);
    if (status) search.set('status', status);
    search.set('page', '1');
    search.set('pageSize', '100');
    return search;
  }, [documentId, key, status]);

  const { data, isLoading } = useQuery({
    queryKey: ['project-history', projectId, params.toString()],
    queryFn: () => getProjectHistory(projectId, params),
  });

  return (
    <div className="p-3 h-full overflow-y-auto bg-slate-50">
      <div className="mx-auto max-w-6xl space-y-1">
        <div>
          <h1 className="text-3xl font-bold text-slate-900">{t('history.title')}</h1>
          <p className="mt-2 text-slate-500">{t('history.subtitle')}</p>
        </div>

        <section className="rounded-[24px] border border-slate-200 bg-white p-2 shadow-sm">
          <div className="grid gap-2 md:grid-cols-3">
            <input value={documentId} onChange={(e) => setDocumentId(e.target.value)} placeholder={t('history.documentFilter')} className="rounded-sm border border-slate-300 px-2 py-1.5" />
            <input value={key} onChange={(e) => setKey(e.target.value)} placeholder={t('history.keyFilter')} className="rounded-sm border border-slate-300 px-2 py-1.5" />
            <select value={status} onChange={(e) => setStatus(e.target.value)} className="rounded-sm border border-slate-300 px-2 py-1.5">
              <option value="">{t('history.allStatuses')}</option>
              <option value="untranslated">{t('workbench.status.untranslated')}</option>
              <option value="translated">{t('workbench.status.translated')}</option>
              <option value="reviewed">{t('workbench.status.reviewed')}</option>
              <option value="approved">{t('workbench.status.approved')}</option>
            </select>
          </div>
        </section>

        <section className="space-y-1">
          {isLoading ? <div className="text-sm text-slate-500">{t('common.loading')}</div> : null}
          {!isLoading && !(data?.items?.length) ? (
            <div className="rounded-[24px] border border-dashed border-slate-300 bg-white px-3 py-16 text-center text-sm text-slate-500">
              {t('history.empty')}
            </div>
          ) : null}
          {data?.items?.map((item) => (
            <article key={item.revisionId} className="rounded-[24px] border border-slate-200 bg-white p-2 shadow-sm">
              <div className="flex flex-wrap items-start justify-between gap-2">
                <div>
                  <div className="text-xs font-semibold uppercase tracking-[0.18em] text-slate-400">{item.documentPath}</div>
                  <div className="mt-2 text-lg font-semibold text-slate-900">{item.key}</div>
                </div>
                <div className="rounded-full bg-slate-100 px-3 py-1 text-xs font-semibold text-slate-600">
                  #{item.revisionNo}
                </div>
              </div>

              <div className="mt-5">
                <RevisionChange item={item} t={t} />
              </div>

              <div className="mt-2 flex flex-wrap items-center gap-1 text-xs text-slate-500">
                <span>{item.changedBy?.name || '-'}</span>
                <span>{new Date(item.changedAt).toLocaleString()}</span>
                {item.changeNote ? <span>{item.changeNote}</span> : null}
              </div>
            </article>
          ))}
        </section>
      </div>
    </div>
  );
}
