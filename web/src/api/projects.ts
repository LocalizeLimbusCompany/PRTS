import { api } from './client';

export interface ProjectSummary {
  id: string;
  name: string;
  slug: string;
  description?: string;
  sourceLanguages: string[];
  targetLanguage: string;
  visibility: 'public' | 'private';
  guestPolicy?: string;
}

export interface ProjectHistoryItem {
  unitId: string;
  key: string;
  documentId: string;
  documentPath: string;
  revisionId: string;
  revisionNo: number;
  beforeTargetText: string;
  afterTargetText: string;
  beforeStatus: string;
  afterStatus: string;
  changeNote: string;
  changedAt: string;
  changedBy?: {
    id: string;
    name: string;
  };
}

export function getProjectsByOrganization(orgId: string) {
  return api.get<{ items: ProjectSummary[]; total: number }>(`/organizations/${orgId}/projects`);
}

export function createProject(orgId: string, payload: {
  name: string;
  slug: string;
  description: string;
  targetLanguage: string;
  sourceLanguages: string[];
  visibility: string;
  guestPolicy: string;
}) {
  return api.post<ProjectSummary>(`/organizations/${orgId}/projects`, payload);
}

export function getProjectHistory(projectId: string, params: URLSearchParams) {
  return api.get<{ items: ProjectHistoryItem[]; total: number; page: number; pageSize: number }>(`/projects/${projectId}/history?${params.toString()}`);
}
