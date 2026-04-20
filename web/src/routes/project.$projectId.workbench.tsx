import { createFileRoute } from '@tanstack/react-router';
import TranslationWorkbench from '@/features/translation/components/TranslationWorkbench';

type WorkbenchSearch = {
  documentId?: string;
};

export const Route = createFileRoute('/project/$projectId/workbench')({
  component: WorkbenchRoute,
  validateSearch: (search: Record<string, unknown>): WorkbenchSearch => {
    return {
      documentId: typeof search.documentId === 'string' ? search.documentId : undefined,
    };
  },
});

function WorkbenchRoute() {
  const { projectId } = Route.useParams();
  const { documentId } = Route.useSearch();
  
  return <TranslationWorkbench projectId={projectId} documentId={documentId} />;
}
