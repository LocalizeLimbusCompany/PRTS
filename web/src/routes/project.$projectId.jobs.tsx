import { createFileRoute } from '@tanstack/react-router'

export const Route = createFileRoute('/project/$projectId/jobs')({
  component: RouteComponent,
})

function RouteComponent() {
  return <div>Hello "/project/$projectId/jobs"!</div>
}
