import { createFileRoute, redirect } from '@tanstack/react-router';
import { useState } from 'react';

import { updateMyPreferences, updateMyProfile, uploadMyAvatar } from '@/api/me';
import { normalizeLocale, translate } from '@/i18n';
import { useAuthStore } from '@/store/auth';
import { usePreferencesStore } from '@/store/preferences';
import { COMMON_LANGUAGE_OPTIONS } from '@/lib/languages';
import { AppShell } from '@/components/AppShell';

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

  return (
    <AppShell title={t('settings.title')} subtitle={t('settings.subtitle')}>
      <div className="max-w-4xl rounded-[30px] border border-slate-200 bg-white p-8 shadow-[0_30px_70px_-45px_rgba(15,23,42,0.35)]">

        <div className="mt-8 space-y-6">
          <div>
            <label className="block text-sm font-semibold text-slate-700 mb-2">{t('settings.nicknameLabel')}</label>
            <input
              value={displayName}
              onChange={(event) => setDisplayName(event.target.value)}
              className="w-full rounded-2xl border border-slate-300 px-4 py-3 text-base outline-none focus:border-blue-500"
            />
            <p className="mt-2 text-sm text-slate-500">{t('settings.nicknameHint')}</p>
          </div>

          <div>
            <label className="block text-sm font-semibold text-slate-700 mb-2">{t('settings.avatarLabel')}</label>
            <div className="flex flex-wrap items-center gap-4">
              <div className="flex h-20 w-20 items-center justify-center overflow-hidden rounded-[22px] bg-slate-100 text-slate-400">
                {avatarUrl ? <img src={avatarUrl} alt={displayName} className="h-full w-full object-cover" /> : displayName.slice(0, 1)}
              </div>
              <div className="flex-1 space-y-3">
                <input
                  value={avatarUrl}
                  onChange={(event) => setAvatarUrl(event.target.value)}
                  className="w-full rounded-2xl border border-slate-300 px-4 py-3 text-base outline-none focus:border-blue-500"
                  placeholder="/uploads/avatar-xxx.png"
                />
                <input
                  type="file"
                  accept="image/png,image/jpeg,image/webp,image/gif"
                  onChange={async (event) => {
                    const file = event.target.files?.[0];
                    if (!file) return;
                    const uploaded = await uploadMyAvatar(file);
                    setAvatarUrl(uploaded.avatarUrl || '');
                    updateUser({ avatarUrl: uploaded.avatarUrl });
                  }}
                  className="block w-full text-sm text-slate-500 file:mr-4 file:rounded-full file:border-0 file:bg-slate-900 file:px-4 file:py-2 file:text-sm file:font-semibold file:text-white"
                />
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
              className="w-full rounded-2xl border border-slate-300 px-4 py-3 text-base outline-none focus:border-blue-500"
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
              className="w-full rounded-2xl border border-slate-300 px-4 py-3 text-base outline-none focus:border-blue-500"
            >
              {COMMON_LANGUAGE_OPTIONS.map((option) => (
                <option key={option.code} value={option.code}>{option.label}</option>
              ))}
            </select>
            <p className="mt-2 text-sm text-slate-500">{t('settings.sourceLanguageHint')}</p>
          </div>
        </div>

        <div className="mt-8 flex items-center gap-4">
          <button
            type="button"
            onClick={handleSave}
            disabled={saving}
            className="rounded-2xl bg-blue-600 px-5 py-3 text-white font-semibold hover:bg-blue-700 disabled:opacity-50"
          >
            {saving ? t('common.saving') : t('common.save')}
          </button>
          {message && <span className="text-sm text-slate-500">{message}</span>}
        </div>
      </div>
    </AppShell>
  );
}
