import { createFileRoute } from '@tanstack/react-router'

export const Route = createFileRoute('/project/$projectId/history')({
  component: RouteComponent,
})

function RouteComponent() {
  return <div>Hello "/project/$projectId/history"!</div>
}
