import { inject, type ComputedRef, type InjectionKey, type Ref } from 'vue'

import type { ProjectDetailDto } from '@/api/types'

/** Shared project data loaded once by the workspace shell. */
export interface ProjectWorkspaceContext {
  detail: Ref<ProjectDetailDto | null>
  loading: Ref<boolean>
  projectId: ComputedRef<number>
  reload: () => Promise<void>
}

/** Injection key used by every project workspace child view. */
export const projectWorkspaceKey: InjectionKey<ProjectWorkspaceContext> = Symbol('projectWorkspace')

/** Fixed information architecture for the project workspace. */
export const PROJECT_WORKSPACE_SECTIONS = [
  { key: 'info', route: 'project-info', icon: 'mdi-information-outline' },
  { key: 'files', route: 'project-files', icon: 'mdi-folder-multiple-outline' },
  { key: 'tasks', route: null, icon: 'mdi-clipboard-text-outline', pending: true },
  { key: 'terms', route: null, icon: 'mdi-book-alphabet', pending: true },
  { key: 'leaderboard', route: 'project-leaderboard', icon: 'mdi-podium' },
  { key: 'download', route: 'project-download', icon: 'mdi-download-outline' },
  { key: 'manage', route: 'project-manage', icon: 'mdi-cog-outline', capability: 'manage_project' },
] as const

/** Require the shell context and fail loudly if a child is mounted outside it. */
export function useProjectWorkspace(): ProjectWorkspaceContext {
  const context = inject(projectWorkspaceKey)
  if (!context) throw new Error('Project workspace context is unavailable')
  return context
}
