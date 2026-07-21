import type { FileDto, FolderDto } from '@/api/types'
import { STATE_ORDER } from '@/lib/states'

export type ProjectFileSort = 'name' | 'progress' | 'entries' | 'updated'

export interface ProjectBrowserItem {
  id: number
  kind: 'folder' | 'file'
  folderId: number | null
  name: string
  path: string
  entryCount: number
  stateCounts: Record<string, number>
  updatedAt: string
  searchText: string
}

export interface ProjectBrowserModel {
  items: ProjectBrowserItem[]
  descendantFileIds: Map<number, number[]>
}

/** Translation progress is undefined for files and folders with no visible entries. */
export function projectFileProgress(item: ProjectBrowserItem): number | null {
  if (item.entryCount === 0) return null
  return (item.entryCount - (item.stateCounts.untranslated ?? 0)) / item.entryCount
}

/** Aggregate all descendant active files into a folder browser row. */
export function projectFolderItem(folder: FolderDto, files: FileDto[]): ProjectBrowserItem {
  const prefix = `${folder.path}/`
  const descendants = files.filter((file) => file.path.startsWith(prefix))
  const stateCounts: Record<string, number> = {}
  let entryCount = 0
  let updatedAt = folder.created_at

  for (const file of descendants) {
    entryCount += file.entry_count
    for (const workflowState of STATE_ORDER) {
      stateCounts[workflowState] =
        (stateCounts[workflowState] ?? 0) + (file.state_counts[workflowState] ?? 0)
    }
    if (file.updated_at > updatedAt) updatedAt = file.updated_at
  }

  return {
    id: folder.id,
    kind: 'folder',
    folderId: folder.parent_id,
    name: folder.name,
    path: folder.path,
    entryCount,
    stateCounts,
    updatedAt,
    searchText: `${folder.name}\u0000${folder.path}`.toLocaleLowerCase(),
  }
}

/** Convert a materialized file DTO into a browser row. */
export function projectFileItem(file: FileDto): ProjectBrowserItem {
  const path = file.path
  return {
    id: file.id,
    kind: 'file',
    folderId: file.folder_id,
    name: file.name,
    path,
    entryCount: file.entry_count,
    stateCounts: file.state_counts,
    updatedAt: file.updated_at,
    searchText: `${file.name}\u0000${path}`.toLocaleLowerCase(),
  }
}

/** Build folder aggregates and task-selection descendants in one bounded tree walk. */
export function buildProjectBrowserModel(
  folders: FolderDto[],
  files: FileDto[],
): ProjectBrowserModel {
  const folderById = new Map(folders.map((folder) => [folder.id, folder]))
  const folderItems = new Map(
    folders.map((folder) => [folder.id, projectFolderItem(folder, [])] as const),
  )
  const descendantFileIds = new Map(folders.map((folder) => [folder.id, [] as number[]] as const))

  for (const file of files) {
    let folderId = file.folder_id
    const visited = new Set<number>()
    while (folderId !== null && !visited.has(folderId)) {
      visited.add(folderId)
      const aggregate = folderItems.get(folderId)
      const folder = folderById.get(folderId)
      if (!aggregate || !folder) break

      aggregate.entryCount += file.entry_count
      for (const workflowState of STATE_ORDER) {
        aggregate.stateCounts[workflowState] =
          (aggregate.stateCounts[workflowState] ?? 0) + (file.state_counts[workflowState] ?? 0)
      }
      if (file.updated_at > aggregate.updatedAt) aggregate.updatedAt = file.updated_at
      descendantFileIds.get(folderId)?.push(file.id)
      folderId = folder.parent_id
    }
  }

  return {
    items: [
      ...folders.flatMap((folder) => {
        const item = folderItems.get(folder.id)
        return item ? [item] : []
      }),
      ...files.map(projectFileItem),
    ],
    descendantFileIds,
  }
}

/** Filter a pre-indexed browser model without repeating locale normalization per query. */
export function filterProjectBrowserItems(
  items: ProjectBrowserItem[],
  query: string,
  folderId: number | null,
  state: string,
): ProjectBrowserItem[] {
  const normalizedQuery = query.trim().toLocaleLowerCase()
  return items.filter((item) => {
    const inFolder = normalizedQuery ? true : item.folderId === folderId
    const matchesName = normalizedQuery ? item.searchText.includes(normalizedQuery) : true
    const matchesState =
      state === 'all' ||
      (state === 'complete'
        ? item.entryCount > 0 && (item.stateCounts.untranslated ?? 0) === 0
        : (item.stateCounts[state] ?? 0) > 0)
    return inFolder && matchesName && matchesState
  })
}

/** Sort folders before files and apply the selected stable secondary ordering. */
export function sortProjectFileItems(
  items: ProjectBrowserItem[],
  sort: ProjectFileSort,
): ProjectBrowserItem[] {
  return [...items].sort((left, right) => {
    if (left.kind !== right.kind) return left.kind === 'folder' ? -1 : 1
    if (sort === 'entries') {
      return right.entryCount - left.entryCount || left.name.localeCompare(right.name)
    }
    if (sort === 'updated') {
      return right.updatedAt.localeCompare(left.updatedAt) || left.name.localeCompare(right.name)
    }
    if (sort === 'progress') {
      return (
        (projectFileProgress(right) ?? -1) - (projectFileProgress(left) ?? -1) ||
        left.name.localeCompare(right.name)
      )
    }
    return left.name.localeCompare(right.name)
  })
}
