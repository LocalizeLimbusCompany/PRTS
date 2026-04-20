import { createFileRoute, redirect } from '@tanstack/react-router';

export const Route = createFileRoute('/project/$projectId/settings')({
  beforeLoad: () => {
    if (!localStorage.getItem('auth-storage') || !JSON.parse(localStorage.getItem('auth-storage') || '{}').state?.token) {
      throw redirect({ to: '/login' });
    }
  },
  component: Settings,
});

function Settings() {
  return (
    <div className="p-6">
      <h1 className="text-2xl font-bold mb-4">Project Settings</h1>
      <p className="text-slate-500">Settings and permissions management coming soon.</p>
    </div>
  );
}