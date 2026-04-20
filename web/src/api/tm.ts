import { api } from './client';

export interface TMEntry {
  id: string;
  sourceLanguage: string;
  targetLanguage: string;
  sourceText: string;
  targetText: string;
  qualityScore: number;
  createdAt: string;
  updatedAt: string;
}

export function getTMEntries(projectId: string) {
  return api.get<{ items: TMEntry[]; total: number }>(`/projects/${projectId}/tm`);
}

export function createTMEntry(projectId: string, data: Partial<TMEntry>) {
  return api.post<TMEntry>(`/projects/${projectId}/tm`, data);
}

export function updateTMEntry(projectId: string, entryId: string, data: Partial<TMEntry>) {
  return api.patch<TMEntry>(`/projects/${projectId}/tm/${entryId}`, data);
}

export function deleteTMEntry(projectId: string, entryId: string) {
  return api.delete(`/projects/${projectId}/tm/${entryId}`);
}
