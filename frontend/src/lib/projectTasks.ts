import type { FileDto, FolderDto } from '@/api/types'

/** Product progress semantics: an empty baseline is complete and requires no work. */
export function taskProgressPercent(denominator: number, completed: number): number {
  if (denominator === 0) return 100
  return Math.round((completed / denominator) * 100)
}

/** Expand a folder selection through the current active folder tree into descendant file IDs. */
export function descendantTaskFileIds(
  folderId: number,
  folders: FolderDto[],
  files: FileDto[],
): number[] {
  const descendantFolders = new Set([folderId])
  let changed = true
  while (changed) {
    changed = false
    for (const folder of folders) {
      if (
        folder.parent_id !== null &&
        descendantFolders.has(folder.parent_id) &&
        !descendantFolders.has(folder.id)
      ) {
        descendantFolders.add(folder.id)
        changed = true
      }
    }
  }
  return files
    .filter((file) => file.folder_id !== null && descendantFolders.has(file.folder_id))
    .map((file) => file.id)
    .sort((left, right) => left - right)
}

/** Toggle a file or an expanded folder against the complete desired task file set. */
export function toggleTaskFileSelection(
  selectedFileIds: number[],
  affectedFileIds: number[],
  selected: boolean,
): number[] {
  const next = new Set(selectedFileIds)
  for (const fileId of affectedFileIds) {
    if (selected) next.add(fileId)
    else next.delete(fileId)
  }
  return [...next].sort((left, right) => left - right)
}
