import { api } from './client';

export interface ProjectMember {
  id: string;
  user: {
    id: string;
    name: string;
  };
  roleCode: string;
  createdAt: string;
}

export function getProjectMembers(projectId: string) {
  return api.get<{ items: ProjectMember[]; total: number }>(`/projects/${projectId}/members`);
}

export function upsertProjectMember(projectId: string, userId: string, roleCode: string) {
  return api.post<{ ok: boolean }>(`/projects/${projectId}/members`, { userId, roleCode });
}

export function deleteProjectMember(projectId: string, userId: string) {
  return api.delete(`/projects/${projectId}/members/${userId}`);
}

export function getProjectRoles(projectId: string) {
  return api.get<{ items: { code: string; name: string; description: string }[] }>(`/projects/${projectId}/roles`);
}