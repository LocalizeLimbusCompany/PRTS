import { createFileRoute } from '@tanstack/react-router'

export const Route = createFileRoute('/project/$projectId/members')({
  component: RouteComponent,
})

function RouteComponent() {
  return <div>Hello "/project/$projectId/members"!</div>
}
