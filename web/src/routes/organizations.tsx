import { createFileRoute, Link, redirect } from '@tanstack/react-router';
import { useQuery } from '@tanstack/react-query';
import { api } from '@/api/client';
import { Building2 } from 'lucide-react';

export const Route = createFileRoute('/organizations')({
  beforeLoad: () => {
    if (!localStorage.getItem('prts_token')) {
      throw redirect({ to: '/login' });
    }
  },
  component: Organizations,
});

interface Organization {
  id: string;
  name: string;
  slug: string;
}

function Organizations() {
  const { data: orgs, isLoading, error } = useQuery({
    queryKey: ['organizations'],
    queryFn: () => api.get<{ items: Organization[] }>('/organizations'),
  });

  return (
    <div className="min-h-screen bg-slate-50 flex items-center justify-center p-4">
      <div className="bg-white max-w-3xl w-full p-8 rounded-lg shadow-sm border border-slate-200">
        <h1 className="text-2xl font-semibold text-slate-900 mb-6 flex items-center">
          <Building2 className="mr-2" />
          Select Organization
        </h1>
        
        {isLoading && <div className="text-slate-500">Loading organizations...</div>}
        {error && <div className="text-red-500">Failed to load organizations.</div>}
        
        <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
          {orgs?.items?.map((org) => (
            <Link
              key={org.id}
              to="/$orgId/projects"
              params={{ orgId: org.id }}
              className="block p-6 border border-slate-200 rounded hover:border-blue-500 hover:shadow-sm transition-all"
            >
              <h2 className="text-lg font-medium text-slate-900">{org.name}</h2>
              <p className="text-sm text-slate-500 mt-1">{org.slug}</p>
            </Link>
          ))}
        </div>
        {orgs?.items?.length === 0 && (
          <div className="text-slate-500">No organizations found.</div>
        )}
      </div>
    </div>
  );
}