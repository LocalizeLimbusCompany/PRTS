import { createFileRoute } from '@tanstack/react-router';

export const Route = createFileRoute('/project/$projectId/glossary')({
  component: Glossary,
});

function Glossary() {
  return (
    <div className="p-6 h-full overflow-y-auto">
      <div className="max-w-5xl mx-auto space-y-6">
        <h1 className="text-2xl font-bold">Glossary</h1>
        <p className="text-slate-500">Project terminology coming soon.</p>
      </div>
    </div>
  );
}