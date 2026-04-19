import { createFileRoute } from '@tanstack/react-router';
import { useQuery } from '@tanstack/react-query';
import { api } from '@/api/client';

export const Route = createFileRoute('/project/$projectId/dashboard')({
  component: Dashboard,
});

function Dashboard() {
  const { projectId } = Route.useParams();
  
  const { data: project, isLoading } = useQuery({
    queryKey: ['project', projectId],
    queryFn: () => api.get<any>(`/projects/${projectId}`),
  });

  return (
    <div className="p-6 h-full overflow-y-auto">
      <div className="max-w-5xl mx-auto space-y-6">
        <h1 className="text-2xl font-bold">Project Overview</h1>
        
        {isLoading ? (
          <div>Loading...</div>
        ) : (
          <div className="grid grid-cols-1 md:grid-cols-3 gap-6">
            <div className="bg-white p-5 rounded border border-slate-200 shadow-sm">
              <h3 className="text-sm font-semibold text-slate-500 mb-2">Basic Info</h3>
              <p className="text-lg font-medium">{project?.name}</p>
              <p className="text-sm text-slate-500">Slug: {project?.slug}</p>
              <div className="mt-4 pt-4 border-t border-slate-100 flex justify-between">
                <div>
                  <div className="text-xs text-slate-400">Sources</div>
                  <div className="font-mono text-sm">{project?.sourceLanguages?.join(', ')}</div>
                </div>
                <div className="text-right">
                  <div className="text-xs text-slate-400">Target</div>
                  <div className="font-mono text-sm text-blue-600">{project?.targetLanguage}</div>
                </div>
              </div>
            </div>
            
            <div className="bg-white p-5 rounded border border-slate-200 shadow-sm md:col-span-2 flex items-center justify-center text-slate-400">
              Statistics & Recent Activity (Placeholder)
            </div>
          </div>
        )}
      </div>
    </div>
  );
}