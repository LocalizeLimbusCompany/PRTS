import { api } from './client';

export interface OrganizationSummary {
  id: string;
  name: string;
  slug: string;
  description?: string;
  visibility: string;
  createdBy?: string;
  canCreateProject?: boolean;
  createRestrictedReason?: string;
}

export function getOrganizations() {
  return api.get<{ items: OrganizationSummary[]; total: number }>('/organizations');
}

export function getOrganization(organizationId: string) {
  return api.get<OrganizationSummary>(`/organizations/${organizationId}`);
}

export function createOrganization(payload: {
  name: string;
  slug: string;
  description: string;
  visibility: string;
}) {
  return api.post<OrganizationSummary>('/organizations', payload);
}
