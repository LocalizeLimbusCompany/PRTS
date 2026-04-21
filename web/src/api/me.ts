import { api } from '@/api/client';
import type { User } from '@/store/auth';

export function getMe() {
  return api.get<User>('/me');
}

export function updateMyPreferences(payload: { preferredLocale: string; preferredSourceLanguage: string }) {
  return api.patch<User>('/me/preferences', payload);
}

export function updateMyProfile(payload: { displayName: string; avatarUrl?: string }) {
  return api.patch<User>('/me', payload);
}

export function uploadMyAvatar(file: File) {
  const formData = new FormData();
  formData.append('avatar', file);
  return api.post<User>('/me/avatar', formData, {
    headers: { 'Content-Type': 'multipart/form-data' },
  });
}
