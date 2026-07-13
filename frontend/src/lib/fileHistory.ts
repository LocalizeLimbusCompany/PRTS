import type { FileChangeItemDto, FileChangeSetDto } from '@/api/types'

export type FileHistoryTarget = { kind: 'file'; id: number } | { kind: 'folder'; id: number }

/** Resolve the immutable target snapshot retained by a change set. */
export function fileHistoryTarget(changeSet: FileChangeSetDto): FileHistoryTarget | null {
  if (changeSet.file_id !== null) return { kind: 'file', id: changeSet.file_id }
  if (changeSet.folder_id !== null) return { kind: 'folder', id: changeSet.folder_id }
  return null
}

/** Restore is valid only for a deletion operation whose target still exists during retention. */
export function canRestoreFileChangeSet(changeSet: FileChangeSetDto): boolean {
  return changeSet.operation === 'delete' && fileHistoryTarget(changeSet) !== null
}

/** Return safe field names changed by one allowlisted delta without exposing their values. */
export function fileHistoryChangedFields(item: FileChangeItemDto): string[] {
  const before = item.before ?? {}
  const after = item.after ?? {}
  return [...new Set([...Object.keys(before), ...Object.keys(after)])]
    .filter((key) => JSON.stringify(before[key]) !== JSON.stringify(after[key]))
    .sort()
}
