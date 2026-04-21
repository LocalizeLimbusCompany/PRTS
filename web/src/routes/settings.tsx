import { createFileRoute, redirect } from '@tanstack/react-router';
import { useState } from 'react';

import { AvatarCropDialog, MAX_AVATAR_BYTES } from '@/components/AvatarCropDialog';
import { AppShell } from '@/components/AppShell';
import { updateMyPreferences, updateMyProfile, uploadMyAvatar } from '@/api/me';
import { normalizeLocale, translate } from '@/i18n';
import { COMMON_LANGUAGE_OPTIONS } from '@/lib/languages';
import { useAuthStore } from '@/store/auth';
import { usePreferencesStore } from '@/store/preferences';

export const Route = createFileRoute('/settings')({
  beforeLoad: () => {
    if (!localStorage.getItem('auth-storage') || !JSON.parse(localStorage.getItem('auth-storage') || '{}').state?.token) {
      throw redirect({ to: '/login' });
    }
  },
  component: UserSettings,
});

function UserSettings() {
  const user = useAuthStore((s) => s.user);
  const updateUser = useAuthStore((s) => s.updateUser);
  const uiLocale = usePreferencesStore((s) => s.uiLocale);
  const setUiLocale = usePreferencesStore((s) => s.setUiLocale);
  const locale = normalizeLocale(user?.preferredLocale || uiLocale);

  const [displayName, setDisplayName] = useState(user?.displayName || '');
  const [avatarUrl, setAvatarUrl] = useState(user?.avatarUrl || '');
  const [preferredLocale, setPreferredLocale] = useState(locale);
  const [preferredSourceLanguage, setPreferredSourceLanguage] = useState(user?.preferredSourceLanguage || 'en');
  const [saving, setSaving] = useState(false);
  const [avatarUploading, setAvatarUploading] = useState(false);
  const [avatarFile, setAvatarFile] = useState<File | null>(null);
  const [avatarError, setAvatarError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);

  const t = (key: string) => translate(preferredLocale, key);

  const handleSave = async () => {
    if (!user) {
      return;
    }

    setSaving(true);
    setMessage(null);

    try {
      const [updatedProfile, updatedPreferences] = await Promise.all([
        updateMyProfile({ displayName, avatarUrl }),
        updateMyPreferences({
          preferredLocale,
          preferredSourceLanguage,
        }),
      ]);

      updateUser({
        displayName: updatedProfile.displayName,
        avatarUrl: updatedProfile.avatarUrl,
        preferredLocale: updatedPreferences.preferredLocale,
        preferredSourceLanguage: updatedPreferences.preferredSourceLanguage,
      });
      setUiLocale(updatedPreferences.preferredLocale);
      setPreferredLocale(normalizeLocale(updatedPreferences.preferredLocale));
      setMessage(t('settings.success'));
    } catch {
      setMessage(t('settings.failed'));
    } finally {
      setSaving(false);
    }
  };

  const handleAvatarFileSelect = (file?: File | null) => {
    setAvatarError(null);
    if (!file) {
      return;
    }
    if (!file.type.startsWith('image/')) {
      setAvatarError(t('settings.avatarInvalid'));
      return;
    }
    if (file.size > MAX_AVATAR_BYTES) {
      setAvatarError(t('settings.avatarTooLarge'));
      return;
    }
    setAvatarFile(file);
  };

  const handleAvatarConfirm = async (blob: Blob) => {
    try {
      setAvatarUploading(true);
      setAvatarError(null);
      setMessage(null);

      const extension = blob.type === 'image/jpeg' ? '.jpg' : '.png';
      const uploadFile = new File([blob], `avatar${extension}`, { type: blob.type });
      const uploaded = await uploadMyAvatar(uploadFile);
      setAvatarUrl(uploaded.avatarUrl || '');
      updateUser({ avatarUrl: uploaded.avatarUrl });
      setAvatarFile(null);
      setMessage(t('settings.avatarUpdated'));
    } catch (error: any) {
      setAvatarError(error?.message || t('settings.avatarUploadFailed'));
    } finally {
      setAvatarUploading(false);
    }
  };

  return (
    <AppShell title={t('settings.title')} subtitle={t('settings.subtitle')}>
      <div className="max-w-4xl rounded-[30px] border border-slate-200 bg-white p-3 shadow-[0_30px_70px_-45px_rgba(15,23,42,0.35)]">

        <div className="mt-3 space-y-1">
          <div>
            <label className="block text-sm font-semibold text-slate-700 mb-2">{t('settings.nicknameLabel')}</label>
            <input
              value={displayName}
              onChange={(event) => setDisplayName(event.target.value)}
              className="w-full rounded-sm border border-slate-300 px-2 py-1.5 text-sm outline-none focus:border-blue-500"
            />
            <p className="mt-2 text-sm text-slate-500">{t('settings.nicknameHint')}</p>
          </div>

          <div>
            <label className="block text-sm font-semibold text-slate-700 mb-2">{t('settings.avatarLabel')}</label>
            <div className="rounded-[26px] border border-slate-200 bg-slate-50 p-2">
              <div className="flex flex-wrap items-center gap-2">
                <div className="flex h-24 w-24 items-center justify-center overflow-hidden rounded-[26px] bg-slate-200 text-2xl font-semibold text-slate-500">
                  {avatarUrl ? <img src={avatarUrl} alt={displayName} className="h-full w-full object-cover" /> : (displayName.slice(0, 1) || '?')}
                </div>
                <div className="flex-1 space-y-1">
                  <div className="text-sm leading-tight text-slate-600">{t('settings.avatarHint')}</div>
                  <input
                    type="file"
                    accept="image/*"
                    onChange={(event) => {
                      const file = event.target.files?.[0];
                      handleAvatarFileSelect(file);
                      event.currentTarget.value = '';
                    }}
                    className="block w-full text-sm text-slate-500 file:mr-4 file:rounded-full file:border-0 file:bg-slate-900 file:px-2 file:py-2 file:text-sm file:font-semibold file:text-white"
                  />
                  {avatarError ? <div className="text-sm text-rose-600">{avatarError}</div> : null}
                </div>
              </div>
            </div>
          </div>

          <div>
            <label className="block text-sm font-semibold text-slate-700 mb-2">{t('settings.languageLabel')}</label>
            <select
              value={preferredLocale}
              onChange={(event) => {
                const nextLocale = normalizeLocale(event.target.value);
                setPreferredLocale(nextLocale);
                setUiLocale(nextLocale);
              }}
              className="w-full rounded-sm border border-slate-300 px-2 py-1.5 text-sm outline-none focus:border-blue-500"
            >
              <option value="zh-CN">{t('settings.zhCN')}</option>
              <option value="en-US">{t('settings.enUS')}</option>
            </select>
            <p className="mt-2 text-sm text-slate-500">{t('settings.languageHint')}</p>
          </div>

          <div>
            <label className="block text-sm font-semibold text-slate-700 mb-2">{t('settings.sourceLanguageLabel')}</label>
            <select
              value={preferredSourceLanguage}
              onChange={(event) => setPreferredSourceLanguage(event.target.value)}
              className="w-full rounded-sm border border-slate-300 px-2 py-1.5 text-sm outline-none focus:border-blue-500"
            >
              {COMMON_LANGUAGE_OPTIONS.map((option) => (
                <option key={option.code} value={option.code}>{option.label}</option>
              ))}
            </select>
            <p className="mt-2 text-sm text-slate-500">{t('settings.sourceLanguageHint')}</p>
          </div>
        </div>

        <div className="mt-3 flex items-center gap-2">
          <button
            type="button"
            onClick={handleSave}
            disabled={saving}
            className="rounded-sm bg-blue-600 px-3 py-3 text-white font-semibold hover:bg-blue-700 disabled:opacity-50"
          >
            {saving ? t('common.saving') : t('common.save')}
          </button>
          {message && <span className="text-sm text-slate-500">{message}</span>}
        </div>
      </div>
      {avatarFile ? (
        <AvatarCropDialog
          file={avatarFile}
          busy={avatarUploading}
          error={avatarError}
          t={t}
          onCancel={() => {
            if (avatarUploading) {
              return;
            }
            setAvatarFile(null);
            setAvatarError(null);
          }}
          onConfirm={handleAvatarConfirm}
        />
      ) : null}
    </AppShell>
  );
}
