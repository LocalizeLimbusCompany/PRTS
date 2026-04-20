import { createFileRoute, redirect } from '@tanstack/react-router';

export const Route = createFileRoute('/project/$projectId/members')({
  beforeLoad: () => {
    if (!localStorage.getItem('auth-storage') || !JSON.parse(localStorage.getItem('auth-storage') || '{}').state?.token) {
      throw redirect({ to: '/login' });
    }
  },
  component: Members,
});

function Members() {
  return (
    <div className="p-6 h-full overflow-y-auto">
      <div className="max-w-5xl mx-auto space-y-6">
        <h1 className="text-2xl font-bold">Members & Permissions</h1>
        <p className="text-slate-500">Member and role management coming soon.</p>
      </div>
    </div>
  );
}