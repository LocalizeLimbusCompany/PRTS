<script setup lang="ts">
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRouter } from 'vue-router'

import type { FileDto, FolderDto } from '@/api/types'
import {
  projectFileItem,
  projectFileProgress,
  projectFolderItem,
  sortProjectFileItems,
  type ProjectBrowserItem,
  type ProjectFileSort,
} from '@/lib/projectFiles'
import { descendantTaskFileIds, toggleTaskFileSelection } from '@/lib/projectTasks'
import { STATE_ORDER } from '@/lib/states'

const props = defineProps<{
  projectId: number
  folders: FolderDto[]
  files: FileDto[]
  canManage?: boolean
  canViewHistory?: boolean
  selectable?: boolean
  selectedFileIds?: number[]
}>()

const emit = defineEmits<{
  createFolder: [parentId: number | null]
  move: [item: ProjectBrowserItem]
  history: [item: ProjectBrowserItem]
  delete: [item: ProjectBrowserItem]
  selectionChange: [fileIds: number[]]
}>()

const router = useRouter()
const { t } = useI18n()
const query = ref('')
const currentFolderId = ref<number | null>(null)
const sort = ref<ProjectFileSort>('name')
const state = ref('all')
const selected = computed(() => new Set(props.selectedFileIds ?? []))

const folderById = computed(() => new Map(props.folders.map((folder) => [folder.id, folder])))

const items = computed(() => {
  const normalizedQuery = query.value.trim().toLocaleLowerCase()
  const all = [
    ...props.folders.map((folder) => projectFolderItem(folder, props.files)),
    ...props.files.map(projectFileItem),
  ].filter((item) => {
    const inFolder = normalizedQuery ? true : item.folderId === currentFolderId.value
    const matchesName = normalizedQuery
      ? item.name.toLocaleLowerCase().includes(normalizedQuery) ||
        item.path.toLocaleLowerCase().includes(normalizedQuery)
      : true
    const matchesState =
      state.value === 'all' ||
      (state.value === 'complete'
        ? item.entryCount > 0 && (item.stateCounts.untranslated ?? 0) === 0
        : (item.stateCounts[state.value] ?? 0) > 0)
    return inFolder && matchesName && matchesState
  })

  return sortProjectFileItems(all, sort.value)
})

const breadcrumbs = computed(() => {
  const result: FolderDto[] = []
  let folder =
    currentFolderId.value === null ? undefined : folderById.value.get(currentFolderId.value)
  while (folder) {
    result.unshift(folder)
    folder = folder.parent_id === null ? undefined : folderById.value.get(folder.parent_id)
  }
  return result
})

const stateOptions = computed(() => [
  { label: t('project.files.allStates'), value: 'all' },
  ...STATE_ORDER.map((value) => ({ label: t(`project.states.${value}`), value })),
  { label: t('project.files.complete'), value: 'complete' },
])

const sortOptions = computed(() => [
  { label: t('project.files.sortName'), value: 'name' },
  { label: t('project.files.sortProgress'), value: 'progress' },
  { label: t('project.files.sortEntries'), value: 'entries' },
  { label: t('project.files.sortUpdated'), value: 'updated' },
])

function progressPercent(item: ProjectBrowserItem): string {
  const value = projectFileProgress(item)
  return value === null ? '—' : `${Math.round(value * 100)}%`
}

function open(item: ProjectBrowserItem) {
  if (item.kind === 'folder') {
    currentFolderId.value = item.id
    query.value = ''
    return
  }
  router.push({ name: 'editor', params: { id: props.projectId }, query: { file: item.id } })
}

function affectedFileIds(item: ProjectBrowserItem): number[] {
  return item.kind === 'file'
    ? [item.id]
    : descendantTaskFileIds(item.id, props.folders, props.files)
}

function selectionState(item: ProjectBrowserItem): boolean | null {
  const affected = affectedFileIds(item)
  if (affected.length === 0) return false
  const selectedCount = affected.filter((fileId) => selected.value.has(fileId)).length
  if (selectedCount === 0) return false
  if (selectedCount === affected.length) return true
  return null
}

function toggleSelection(item: ProjectBrowserItem, value: boolean | null) {
  emit(
    'selectionChange',
    toggleTaskFileSelection(props.selectedFileIds ?? [], affectedFileIds(item), value === true),
  )
}

function activate(item: ProjectBrowserItem) {
  if (props.selectable && item.kind === 'file') {
    const next = selectionState(item) !== true
    toggleSelection(item, next)
    return
  }
  open(item)
}
</script>

<template>
  <section class="file-browser">
    <div class="file-browser__toolbar">
      <q-input v-model="query" dense outlined clearable :placeholder="$t('project.files.search')">
        <template #prepend><q-icon name="mdi-magnify" /></template>
      </q-input>
      <q-select
        v-model="state"
        dense
        outlined
        emit-value
        map-options
        :options="stateOptions"
        :label="$t('project.files.state')"
      />
      <q-select
        v-model="sort"
        dense
        outlined
        emit-value
        map-options
        :options="sortOptions"
        :label="$t('project.files.sort')"
      />
      <q-btn
        v-if="canManage"
        outline
        no-caps
        icon="mdi-folder-plus-outline"
        :label="$t('project.files.createFolder')"
        @click="emit('createFolder', currentFolderId)"
      />
    </div>

    <div class="file-browser__breadcrumbs prts-mono">
      <button type="button" @click="currentFolderId = null">{{ $t('project.files.root') }}</button>
      <template v-for="folder in breadcrumbs" :key="folder.id">
        <q-icon name="mdi-chevron-right" />
        <button type="button" @click="currentFolderId = folder.id">{{ folder.name }}</button>
      </template>
    </div>

    <div class="file-browser__head prts-label">
      <span>{{ $t('project.files.name') }}</span>
      <span>{{ $t('project.progress') }}</span>
      <span>{{ $t('project.entries') }}</span>
      <span>{{ $t('project.files.updated') }}</span>
      <span v-if="canManage || canViewHistory">{{ $t('project.files.actions') }}</span>
    </div>
    <div v-if="items.length === 0" class="prts-empty">{{ $t('project.files.empty') }}</div>
    <template v-else>
      <div
        v-for="item in items"
        :key="`${item.kind}-${item.id}`"
        class="file-browser__row"
        role="button"
        tabindex="0"
        @click="activate(item)"
        @keyup.enter.self="activate(item)"
      >
        <span
          class="file-browser__name"
          :class="{ 'file-browser__name--selectable': selectable }"
        >
          <q-checkbox
            v-if="selectable"
            :model-value="selectionState(item)"
            :indeterminate-value="null"
            keep-color
            color="primary"
            @click.stop
            @update:model-value="toggleSelection(item, $event)"
          />
          <q-icon
            :name="item.kind === 'folder' ? 'mdi-folder-outline' : 'mdi-file-document-outline'"
            :color="item.kind === 'folder' ? 'grey' : 'primary'"
            size="19px"
          />
          <span>
            <strong>{{ item.name }}</strong>
            <small class="prts-mono">{{ item.path }}</small>
          </span>
        </span>
        <span class="file-browser__progress">
          <span class="prts-mono">{{ progressPercent(item) }}</span>
          <q-linear-progress :value="projectFileProgress(item) ?? 0" size="4px" color="primary" />
        </span>
        <span class="prts-mono">{{ item.entryCount }}</span>
        <span class="prts-dim">{{ new Date(item.updatedAt).toLocaleDateString() }}</span>
        <span v-if="canManage || canViewHistory" class="file-browser__actions">
          <q-btn
            v-if="canViewHistory"
            flat
            round
            dense
            icon="mdi-history"
            :aria-label="$t('project.files.history')"
            @click.stop="emit('history', item)"
          >
            <q-tooltip>{{ $t('project.files.history') }}</q-tooltip>
          </q-btn>
          <q-btn
            v-if="canManage"
            flat
            round
            dense
            icon="mdi-file-move-outline"
            :aria-label="$t('project.files.move')"
            @click.stop="emit('move', item)"
          >
            <q-tooltip>{{ $t('project.files.move') }}</q-tooltip>
          </q-btn>
          <q-btn
            v-if="canManage"
            flat
            round
            dense
            color="negative"
            icon="mdi-delete-clock-outline"
            :aria-label="$t('project.files.delete')"
            @click.stop="emit('delete', item)"
          >
            <q-tooltip>{{ $t('project.files.delete') }}</q-tooltip>
          </q-btn>
        </span>
      </div>
    </template>
  </section>
</template>

<style scoped>
.file-browser {
  border: 1px solid var(--prts-border);
  background: var(--prts-panel);
}

.file-browser__toolbar {
  display: grid;
  grid-template-columns: minmax(220px, 1fr) 180px 180px auto;
  gap: 10px;
  padding: 12px;
  border-bottom: 1px solid var(--prts-border);
}

.file-browser__breadcrumbs {
  display: flex;
  align-items: center;
  min-height: 38px;
  padding: 0 14px;
  border-bottom: 1px solid var(--prts-border-soft);
  color: var(--prts-text-dim);
  font-size: 11px;
}

.file-browser__breadcrumbs button {
  border: 0;
  background: transparent;
  color: inherit;
  cursor: pointer;
}

.file-browser__head,
.file-browser__row {
  display: grid;
  grid-template-columns: minmax(240px, 1fr) 160px 92px 120px auto;
  align-items: center;
  gap: 18px;
  padding: 10px 14px;
}

.file-browser__head {
  border-bottom: 1px solid var(--prts-border);
  background: var(--prts-panel-2);
}

.file-browser__row {
  width: 100%;
  min-height: 60px;
  border: 0;
  border-bottom: 1px solid var(--prts-border-soft);
  background: transparent;
  color: var(--prts-text);
  font: inherit;
  text-align: left;
  cursor: pointer;
}

.file-browser__row:hover {
  background: var(--prts-panel-2);
}

.file-browser__name,
.file-browser__name > span,
.file-browser__progress {
  display: grid;
}

.file-browser__name {
  grid-template-columns: 22px minmax(0, 1fr);
  align-items: center;
  gap: 9px;
}

.file-browser__name--selectable {
  grid-template-columns: auto 22px minmax(0, 1fr);
}

.file-browser__name strong,
.file-browser__name small {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.file-browser__name small {
  color: var(--prts-text-faint);
  font-size: 10px;
}

.file-browser__progress {
  gap: 5px;
}

.file-browser__actions {
  display: flex;
  justify-content: flex-end;
  min-width: 104px;
}

@media (max-width: 980px) {
  .file-browser__toolbar {
    grid-template-columns: 1fr 1fr;
  }

  .file-browser__toolbar :first-child {
    grid-column: 1 / -1;
  }

  .file-browser__head,
  .file-browser__row {
    grid-template-columns: minmax(180px, 1fr) 120px 70px auto;
  }

  .file-browser__head > :nth-child(4),
  .file-browser__row > :nth-child(4) {
    display: none;
  }
}
</style>
