import { api } from './client';
import type { User } from '@/store/auth';

export interface PlatformSettings {
  allowUserCreateOrganization: boolean;
  allowUserCreateProject: boolean;
  updatedAt: string;
  updatedBy?: {
    id: string;
    name: string;
  };
}

export interface PlatformOverview {
  userCount: number;
  organizationCount: number;
  projectCount: number;
  settings: PlatformSettings;
}

export function getPlatformOverview() {
  return api.get<PlatformOverview>('/admin/overview');
}

export function getPlatformSettings() {
  return api.get<PlatformSettings>('/admin/settings');
}

export function updatePlatformSettings(payload: Pick<PlatformSettings, 'allowUserCreateOrganization' | 'allowUserCreateProject'>) {
  return api.patch<PlatformSettings>('/admin/settings', payload);
}

export function getPlatformUsers() {
  return api.get<{ items: User[]; total: number }>('/admin/users');
}

export function updatePlatformUser(userId: string, payload: { platformRole: string; status: string }) {
  return api.patch<User>(`/admin/users/${userId}`, payload);
}
