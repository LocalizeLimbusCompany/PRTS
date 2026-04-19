import { createFileRoute } from '@tanstack/react-router';

export const Route = createFileRoute('/project/$projectId/tm')({
  component: TranslationMemory,
});

function TranslationMemory() {
  return (
    <div className="p-6 h-full overflow-y-auto">
      <div className="max-w-5xl mx-auto space-y-6">
        <h1 className="text-2xl font-bold">Translation Memory</h1>
        <p className="text-slate-500">Translation memory management coming soon.</p>
      </div>
    </div>
  );
}