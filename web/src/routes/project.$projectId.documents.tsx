import { createFileRoute } from '@tanstack/react-router';

export const Route = createFileRoute('/project/$projectId/documents')({
  component: Documents,
});

function Documents() {
  return (
    <div className="p-6">
      <h1 className="text-2xl font-bold mb-4">Documents</h1>
      <p className="text-slate-500">Document management coming soon.</p>
    </div>
  );
}