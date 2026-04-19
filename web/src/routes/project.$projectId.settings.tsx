import { createFileRoute } from '@tanstack/react-router';

export const Route = createFileRoute('/project/$projectId/settings')({
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