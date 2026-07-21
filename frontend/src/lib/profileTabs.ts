export const PROFILE_TABS = ['profile', 'ai', 'security', 'api_keys'] as const

export type ProfileTab = (typeof PROFILE_TABS)[number]

/** Keep profile sections addressable while treating unknown or repeated query values as invalid. */
export function resolveProfileTab(value: unknown): ProfileTab {
  return typeof value === 'string' && PROFILE_TABS.includes(value as ProfileTab)
    ? (value as ProfileTab)
    : 'profile'
}
