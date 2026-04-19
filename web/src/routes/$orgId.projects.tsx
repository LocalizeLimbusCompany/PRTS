import { createFileRoute, Link, redirect } from '@tanstack/react-router';
import { useQuery } from '@tanstack/react-query';
import { api } from '@/api/client';
import { FolderKanban, Globe, Lock } from 'lucide-react';

export const Route = createFileRoute('/$orgId/projects')({
  beforeLoad: () => {
    if (!localStorage.getItem('prts_token')) {
      throw redirect({ to: '/login' });
    }
  },
  component: Projects,
});

interface Project {
  id: string;
  name: string;
  slug: string;
  sourceLanguages: string[];
  targetLanguage: string;
  visibility: 'public' | 'private';
}

function Projects() {
  const { orgId } = Route.useParams();
  
  const { data: projects, isLoading, error } = useQuery({
    queryKey: ['projects', orgId],
    queryFn: () => api.get<{ items: Project[] }>(`/organizations/${orgId}/projects`),
  });

  return (
    <div className="min-h-screen bg-slate-50 flex items-center justify-center p-4">
      <div className="bg-white max-w-4xl w-full p-8 rounded-lg shadow-sm border border-slate-200">
        <h1 className="text-2xl font-semibold text-slate-900 mb-6 flex items-center">
          <FolderKanban className="mr-2" />
          Select Project
        </h1>
        
        {isLoading && <div className="text-slate-500">Loading projects...</div>}
        {error && <div className="text-red-500">Failed to load projects.</div>}
        
        <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
          {projects?.items?.map((proj) => (
            <Link
              key={proj.id}
              to="/project/$projectId/workbench"
              params={{ projectId: proj.id }}
              className="block p-6 border border-slate-200 rounded hover:border-blue-500 hover:shadow-sm transition-all"
            >
              <div className="flex justify-between items-start mb-2">
                <h2 className="text-lg font-medium text-slate-900">{proj.name}</h2>
                {proj.visibility === 'public' ? (
                  <Globe className="w-4 h-4 text-slate-400" />
                ) : (
                  <Lock className="w-4 h-4 text-slate-400" />
                )}
              </div>
              <p className="text-sm text-slate-500 mb-4">{proj.slug}</p>
              
              <div className="flex items-center text-xs font-mono text-slate-600 bg-slate-50 p-2 rounded">
                <span className="truncate max-w-[120px]">{proj.sourceLanguages.join(', ')}</span>
                <span className="mx-2 text-slate-400">→</span>
                <span className="font-semibold text-blue-600">{proj.targetLanguage}</span>
              </div>
            </Link>
          ))}
        </div>
        {projects?.items?.length === 0 && (
          <div className="text-slate-500">No projects found.</div>
        )}
        
        <div className="mt-8 pt-6 border-t border-slate-100">
          <Link to="/organizations" className="text-sm text-blue-600 hover:underline">
            ← Back to Organizations
          </Link>
        </div>
      </div>
    </div>
  );
}