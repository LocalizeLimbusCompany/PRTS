import { createFileRoute } from '@tanstack/react-router';
import TranslationWorkbench from '@/features/translation/components/TranslationWorkbench';

type WorkbenchSearch = {
  documentId?: string;
  q?: string;
  status?: string;
  scope?: string;
  questioned?: string;
};

export const Route = createFileRoute('/project/$projectId/workbench')({
  component: WorkbenchRoute,
  validateSearch: (search: Record<string, unknown>): WorkbenchSearch => {
    return {
      documentId: typeof search.documentId === 'string' ? search.documentId : undefined,
      q: typeof search.q === 'string' ? search.q : undefined,
      status: typeof search.status === 'string' ? search.status : undefined,
      scope: typeof search.scope === 'string' ? search.scope : undefined,
      questioned: typeof search.questioned === 'string' ? search.questioned : undefined,
    };
  },
});

function WorkbenchRoute() {
  const { projectId } = Route.useParams();
  const { documentId } = Route.useSearch();
  
  return <TranslationWorkbench projectId={projectId} documentId={documentId} />;
}
