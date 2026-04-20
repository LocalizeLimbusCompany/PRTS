import { api } from './client';

export interface GlossaryTerm {
  id: string;
  sourceTerm: string;
  targetTerm: string;
  sourceLanguage: string;
  targetLanguage: string;
  note: string;
  createdAt: string;
  updatedAt: string;
}

export function getGlossaryTerms(projectId: string) {
  return api.get<{ items: GlossaryTerm[]; total: number }>(`/projects/${projectId}/glossary`);
}

export function createGlossaryTerm(projectId: string, data: Partial<GlossaryTerm>) {
  return api.post<GlossaryTerm>(`/projects/${projectId}/glossary`, data);
}

export function updateGlossaryTerm(projectId: string, termId: string, data: Partial<GlossaryTerm>) {
  return api.patch<GlossaryTerm>(`/projects/${projectId}/glossary/${termId}`, data);
}

export function deleteGlossaryTerm(projectId: string, termId: string) {
  return api.delete(`/projects/${projectId}/glossary/${termId}`);
}
