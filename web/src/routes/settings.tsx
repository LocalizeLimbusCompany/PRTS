import { createFileRoute, redirect } from '@tanstack/react-router';
import { useState } from 'react';

import { updateMyPreferences, updateMyProfile } from '@/api/me';
import { normalizeLocale, translate } from '@/i18n';
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
  const [preferredLocale, setPreferredLocale] = useState(locale);
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
        updateMyProfile({ displayName }),
        updateMyPreferences({
          preferredLocale,
          preferredSourceLanguage: user.preferredSourceLanguage || '',
        }),
      ]);

      updateUser({
        displayName: updatedProfile.displayName,
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
    <div className="p-6 h-full overflow-y-auto bg-slate-50">
      <div className="max-w-3xl mx-auto rounded-3xl border border-slate-200 bg-white p-8 shadow-sm">
        <h1 className="text-3xl font-semibold text-slate-900">{t('settings.title')}</h1>
        <p className="mt-2 text-slate-500">{t('settings.subtitle')}</p>

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
    </div>
  );
}
