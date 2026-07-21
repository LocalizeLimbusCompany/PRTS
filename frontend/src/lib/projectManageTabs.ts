export type ProjectManageTab = 'basic' | 'ai' | 'language' | 'join' | 'members' | 'danger'

export interface ProjectManageTabAccess {
  manageProject: boolean
  owner: boolean
  manageMembers: boolean
  deleteProject: boolean
  deletionPending: boolean
}

/** Return only sections authorized by server-authored capabilities and owner identity. */
export function availableProjectManageTabs(access: ProjectManageTabAccess): ProjectManageTab[] {
  if (access.deletionPending) return ['danger']
  const tabs: ProjectManageTab[] = []
  if (access.manageProject) tabs.push('basic')
  if (access.owner) tabs.push('ai')
  if (access.manageProject) tabs.push('language')
  if (access.manageProject) tabs.push('join')
  if (access.manageMembers) tabs.push('members')
  if (access.deleteProject) tabs.push('danger')
  return tabs
}

/** Invalid or newly inaccessible query tabs fall back to the first visible section. */
export function resolveProjectManageTab(
  requested: unknown,
  available: ProjectManageTab[],
): ProjectManageTab | null {
  return typeof requested === 'string' && available.includes(requested as ProjectManageTab)
    ? (requested as ProjectManageTab)
    : (available[0] ?? null)
}
