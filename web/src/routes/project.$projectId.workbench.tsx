import { createFileRoute } from '@tanstack/react-router';
import TranslationWorkbench from '@/features/translation/components/TranslationWorkbench';

export const Route = createFileRoute('/project/$projectId/workbench')({
  component: WorkbenchRoute,
});

function WorkbenchRoute() {
  const { projectId } = Route.useParams();
  return <TranslationWorkbench projectId={projectId} />;
}