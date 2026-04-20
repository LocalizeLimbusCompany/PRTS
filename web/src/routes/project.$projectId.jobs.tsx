import { createFileRoute } from '@tanstack/react-router';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { api } from '@/api/client';
import { useState } from 'react';

import { useAuthStore } from '@/store/auth';

export const Route = createFileRoute('/project/$projectId/jobs')({
  component: Jobs,
});

function Jobs() {
  const { projectId } = Route.useParams();
  const queryClient = useQueryClient();
  const [file, setFile] = useState<File | null>(null);
  const user = useAuthStore(s => s.user);

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
        alert("Invalid JSON file.");
      }
    };
    reader.readAsText(file);
  };

  const handleDownload = (downloadUrl: string) => {
    // Navigate to the download URL to trigger browser download
    window.location.href = downloadUrl;
  };

  return (
    <div className="p-6 h-full overflow-y-auto">
      <div className="max-w-5xl mx-auto space-y-8">
        <h1 className="text-2xl font-bold">Import / Export Jobs</h1>
        
        {!user && (
          <div className="bg-blue-50 text-blue-700 p-3 rounded mb-4 text-sm">
            You must be logged in to create new import/export jobs. You can still view the job history.
          </div>
        )}

        <div className="grid grid-cols-1 md:grid-cols-2 gap-8">
          {/* Import Section */}
          <div className="bg-white p-6 rounded shadow-sm border border-slate-200">
            <h2 className="text-lg font-semibold mb-4">Import JSON</h2>
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
                {createImport.isPending ? 'Importing...' : 'Start Import'}
              </button>
            </div>

            <div className="mt-8">
              <h3 className="text-sm font-semibold text-slate-600 mb-2">Recent Imports</h3>
              {isLoadingImports ? <div className="text-sm">Loading...</div> : (
                <ul className="space-y-2 text-sm text-slate-600">
                  {importsData?.items?.map((job: any) => (
                    <li key={job.id} className="flex justify-between border-b pb-2">
                      <span>{new Date(job.createdAt).toLocaleString()}</span>
                      <span className="capitalize">{job.status}</span>
                    </li>
                  ))}
                  {!importsData?.items?.length && <li>No import jobs found.</li>}
                </ul>
              )}
            </div>
          </div>

          {/* Export Section */}
          <div className="bg-white p-6 rounded shadow-sm border border-slate-200">
            <h2 className="text-lg font-semibold mb-4">Export ZIP</h2>
            <button 
              onClick={() => createExport.mutate()}
              disabled={createExport.isPending || !user}
              className="bg-blue-600 text-white px-4 py-2 rounded text-sm disabled:opacity-50 mb-4"
            >
              {createExport.isPending ? 'Requesting Export...' : 'Request New Export'}
            </button>

            <div className="mt-8">
              <h3 className="text-sm font-semibold text-slate-600 mb-2">Recent Exports</h3>
              {isLoadingExports ? <div className="text-sm">Loading...</div> : (
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
                          Download ZIP
                        </button>
                      )}
                    </li>
                  ))}
                  {!exportsData?.items?.length && <li>No export jobs found.</li>}
                </ul>
              )}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}