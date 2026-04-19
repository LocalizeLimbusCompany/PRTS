import { createFileRoute } from '@tanstack/react-router';

export const Route = createFileRoute('/settings')({
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