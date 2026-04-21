import { createFileRoute } from '@tanstack/react-router';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { api } from '@/api/client';
import { useState } from 'react';

import { normalizeLocale, translate } from '@/i18n';
import { useAuthStore } from '@/store/auth';
import { usePreferencesStore } from '@/store/preferences';

export const Route = createFileRoute('/project/$projectId/jobs')({
  component: Jobs,
});

function Jobs() {
  const { projectId } = Route.useParams();
  const queryClient = useQueryClient();
  const [file, setFile] = useState<File | null>(null);
  const user = useAuthStore(s => s.user);
  const uiLocale = usePreferencesStore((s) => s.uiLocale);
  const locale = normalizeLocale(user?.preferredLocale || uiLocale);
  const t = (key: string) => translate(locale, key);

  const { data: exportsData, isLoading: isLoadingExports } = useQuery({
    queryKey: ['exports', projectId],
    queryFn: () => api.get<any>(`/projects/${projectId}/exports`),
  });

  const { data: importsData, isLoading: isLoadingImports } = useQuery({
    queryKey: ['imports', projectId],
    queryFn: () => api.get<any>(`/projects/${projectId}/imports`),
  });

  const createExport = useMutation({
    mutationFn: () => api.post(`/projects/${projectId}/exports`),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['exports', projectId] })
  });

  const createImport = useMutation({
    mutationFn: (jsonData: any) => api.post(`/projects/${projectId}/imports`, jsonData),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['imports', projectId] });
      setFile(null);
    }
  });

  const handleImport = () => {
    if (!file) return;
    const reader = new FileReader();
    reader.onload = (e) => {
      try {
        const json = JSON.parse(e.target?.result as string);
        createImport.mutate(json);
      } catch (err) {
          alert(t('jobs.invalidJson'));
        }
      };
    reader.readAsText(file);
  };

  const handleDownload = (downloadUrl: string) => {
    window.location.href = downloadUrl;
  };

  return (
    <div className="p-6 h-full overflow-y-auto">
      <div className="max-w-5xl mx-auto space-y-8">
        <h1 className="text-2xl font-bold">{t('jobs.title')}</h1>
        
        {!user && (
          <div className="bg-blue-50 text-blue-700 p-3 rounded mb-4 text-sm">
            {t('jobs.loginHint')}
          </div>
        )}

        <div className="grid grid-cols-1 md:grid-cols-2 gap-8">
          {/* Import Section */}
          <div className="bg-white p-6 rounded shadow-sm border border-slate-200">
            <h2 className="text-lg font-semibold mb-4">{t('jobs.importTitle')}</h2>
            <div className="space-y-4">
              <input 
                type="file" 
                accept=".json"
                disabled={!user}
                onChange={(e) => setFile(e.target.files?.[0] || null)}
                className="block w-full text-sm text-slate-500 file:mr-4 file:py-2 file:px-4 file:rounded file:border-0 file:text-sm file:font-semibold file:bg-blue-50 file:text-blue-700 hover:file:bg-blue-100 disabled:opacity-50"
              />
              <button 
                onClick={handleImport}
                disabled={!file || createImport.isPending || !user}
                className="bg-blue-600 text-white px-4 py-2 rounded text-sm disabled:opacity-50"
              >
                  {createImport.isPending ? t('jobs.importLoading') : t('jobs.importAction')}
                </button>
            </div>

            <div className="mt-8">
              <h3 className="text-sm font-semibold text-slate-600 mb-2">{t('jobs.recentImports')}</h3>
              {isLoadingImports ? <div className="text-sm">{t('common.loading')}</div> : (
                <ul className="space-y-2 text-sm text-slate-600">
                  {importsData?.items?.map((job: any) => (
                    <li key={job.id} className="flex justify-between border-b pb-2">
                      <span>{new Date(job.createdAt).toLocaleString()}</span>
                      <span className="capitalize">{job.status}</span>
                    </li>
                  ))}
                  {!importsData?.items?.length && <li>{t('jobs.importEmpty')}</li>}
                </ul>
              )}
            </div>
          </div>

          {/* Export Section */}
          <div className="bg-white p-6 rounded shadow-sm border border-slate-200">
            <h2 className="text-lg font-semibold mb-4">{t('jobs.exportTitle')}</h2>
            <button 
              onClick={() => createExport.mutate()}
              disabled={createExport.isPending || !user}
              className="bg-blue-600 text-white px-4 py-2 rounded text-sm disabled:opacity-50 mb-4"
            >
              {createExport.isPending ? t('jobs.exportLoading') : t('jobs.exportAction')}
            </button>

            <div className="mt-8">
              <h3 className="text-sm font-semibold text-slate-600 mb-2">{t('jobs.recentExports')}</h3>
              {isLoadingExports ? <div className="text-sm">{t('common.loading')}</div> : (
                <ul className="space-y-2 text-sm text-slate-600">
                  {exportsData?.items?.map((job: any) => (
                    <li key={job.id} className="flex flex-col border-b pb-2">
                      <div className="flex justify-between">
                        <span>{job.fileName || `Export ${job.id.substring(0,6)}`}</span>
                        <span className="capitalize font-semibold">{job.status}</span>
                      </div>
                      {job.status === 'finished' && job.downloadUrl && (
                        <button 
                          onClick={() => handleDownload(job.downloadUrl)}
                          className="text-blue-600 text-left mt-1 hover:underline"
                        >
                          {t('jobs.exportDownload')}
                        </button>
                      )}
                    </li>
                  ))}
                  {!exportsData?.items?.length && <li>{t('jobs.exportEmpty')}</li>}
                </ul>
              )}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
