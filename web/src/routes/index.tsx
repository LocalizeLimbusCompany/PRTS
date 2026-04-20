import { createFileRoute } from '@tanstack/react-router';
import { OrganizationsPage } from './organizations';

export const Route = createFileRoute('/')({
  component: OrganizationsPage,
});
