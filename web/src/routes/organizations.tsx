import { createFileRoute } from '@tanstack/react-router';
import { OrganizationsPage } from '@/components/OrganizationsPage';

export const Route = createFileRoute('/organizations')({
  component: OrganizationsPage,
});
