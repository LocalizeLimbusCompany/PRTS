import { useQuery } from '@tanstack/react-query';
import { createFileRoute, useNavigate } from '@tanstack/react-router';
import { useEffect } from 'react';

import { getOrganizations } from '@/api/organizations';
import { OrganizationsPage } from '@/components/OrganizationsPage';

export const Route = createFileRoute('/')({
  component: IndexPage,
});

function IndexPage() {
  const navigate = useNavigate();

  const { data: orgs, isLoading } = useQuery({
    queryKey: ['organizations'],
    queryFn: getOrganizations,
  });

  useEffect(() => {
    const firstOrganization = orgs?.items[0];
    if (orgs?.items.length === 1 && firstOrganization) {
      void navigate({
        to: '/$orgId/projects',
        params: { orgId: firstOrganization.id },
        replace: true,
      });
    }
  }, [navigate, orgs]);

  if (isLoading || orgs?.items.length === 1) {
    return (
      <div className="min-h-screen bg-slate-50 flex items-center justify-center text-sm text-slate-500">
        Loading public projects...
      </div>
    );
  }

  return <OrganizationsPage />;
}
