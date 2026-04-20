import { createFileRoute, redirect } from '@tanstack/react-router';

export const Route = createFileRoute('/settings')({
  beforeLoad: () => {
    if (!localStorage.getItem('auth-storage') || !JSON.parse(localStorage.getItem('auth-storage') || '{}').state?.token) {
      throw redirect({ to: '/login' });
    }
  },
  component: UserSettings,
});

function UserSettings() {
  return (
    <div className="p-6">
      <h1 className="text-2xl font-bold mb-4">User Settings</h1>
      <p className="text-slate-500">User settings coming soon.</p>
    </div>
  );
}