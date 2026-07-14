<script lang="ts">
import type { EntryState, SearchCondition, SearchScope, StructuredSearchRequest } from '@/api'

export type AdvancedScopeType = SearchScope['type']

export interface AdvancedFilterDraft {
  query: string
  conditions: SearchCondition[]
  states: EntryState[]
  includeHidden: boolean
  vector: boolean
  scopeType: AdvancedScopeType
  path?: string
  fileId?: number | null
  currentFileId?: number | null
  taskId?: number | null
}

function positiveId(value: number | null | undefined, field: string): number {
  if (!Number.isInteger(value) || (value ?? 0) <= 0) throw new Error(`${field}_required`)
  return value as number
}

function scopeFromDraft(draft: AdvancedFilterDraft): SearchScope {
  switch (draft.scopeType) {
    case 'all':
      return { type: 'all' }
    case 'path': {
      const path = draft.path?.trim()
      if (!path) throw new Error('path_required')
      return { type: 'path', path }
    }
    case 'file':
      return { type: 'file', file_id: positiveId(draft.fileId, 'file_id') }
    case 'current_file':
      return {
        type: 'current_file',
        file_id: positiveId(draft.currentFileId, 'current_file_id'),
      }
    case 'current_task':
      return { type: 'current_task', task_id: positiveId(draft.taskId, 'task_id') }
  }
}

/** 把显式表单值构造成后端 deny_unknown_fields union，不从 session 猜资源 ID。 */
export function buildAdvancedSearchRequest(draft: AdvancedFilterDraft): StructuredSearchRequest {
  const query = draft.query.trim()
  return {
    ...(query ? { query } : {}),
    conditions: draft.conditions.map((condition) => ({ ...condition })),
    scope: scopeFromDraft(draft),
    states: [...draft.states],
    include_hidden: draft.includeHidden,
    vector: draft.vector,
    limit: 50,
  }
}
</script>

<script setup lang="ts">
import { computed, reactive, watch } from 'vue'
import { useI18n } from 'vue-i18n'

import {
  ENTRY_STATES,
  type FileDto,
  type SearchCondition as Condition,
  type SearchOperator,
  type StructuredSearchRequest as SearchRequest,
} from '@/api'
import { stateLabel } from '@/lib/states'

const props = defineProps<{
  files: FileDto[]
  sourceLangs: string[]
  currentFileId: number | null
  currentTaskId: number | null
  canIncludeHidden: boolean
}>()
const model = defineModel<boolean>({ default: false })
const emit = defineEmits<{ (event: 'search', request: SearchRequest): void }>()
const { t } = useI18n()

const draft = reactive<AdvancedFilterDraft>({
  query: '',
  conditions: [],
  states: [],
  includeHidden: false,
  vector: false,
  scopeType: 'all',
  path: '',
  fileId: null,
  currentFileId: null,
  taskId: null,
})

const scopeOptions = computed(() => [
  { label: t('editor.scopeAll'), value: 'all' },
  { label: t('editor.scopePath'), value: 'path' },
  { label: t('editor.scopeFile'), value: 'file' },
  { label: t('editor.scopeCurrentFile'), value: 'current_file' },
  { label: t('editor.scopeCurrentTask'), value: 'current_task' },
])
const fieldOptions = computed(() => [
  ...props.sourceLangs.map((language) => ({
    label: `${t('editor.fieldSource')} · ${language}`,
    value: `source:${language}`,
  })),
  { label: t('editor.fieldSourceAny'), value: 'source_any' },
  { label: t('editor.fieldTranslation'), value: 'translation' },
  { label: t('editor.fieldKey'), value: 'key' },
])
const operatorOptions: Array<{ label: string; value: SearchOperator }> = [
  { label: t('editor.opContains'), value: 'contains' },
  { label: t('editor.opNotContains'), value: 'not_contains' },
  { label: t('editor.opStartsWith'), value: 'starts_with' },
  { label: t('editor.opEndsWith'), value: 'ends_with' },
  { label: t('editor.opEquals'), value: 'equals' },
]
const stateOptions = ENTRY_STATES.map((state) => ({ label: stateLabel(state), value: state }))
const fileOptions = computed(() =>
  props.files.map((file) => ({ label: file.path, value: file.id })),
)
const scopeReady = computed(() => {
  switch (draft.scopeType) {
    case 'all':
      return true
    case 'path':
      return Boolean(draft.path?.trim())
    case 'file':
      return Number.isInteger(draft.fileId) && (draft.fileId ?? 0) > 0
    case 'current_file':
      return Number.isInteger(draft.currentFileId) && (draft.currentFileId ?? 0) > 0
    case 'current_task':
      return Number.isInteger(draft.taskId) && (draft.taskId ?? 0) > 0
    default:
      return false
  }
})

watch(model, (open) => {
  if (!open) return
  draft.currentFileId = props.currentFileId
  draft.taskId = props.currentTaskId
})

function addCondition() {
  const condition: Condition = {
    field: props.sourceLangs[0] ? `source:${props.sourceLangs[0]}` : 'source_any',
    operator: 'contains',
    value: '',
  }
  draft.conditions.push(condition)
}

function submit() {
  emit('search', buildAdvancedSearchRequest(draft))
  model.value = false
}
</script>

<template>
  <q-dialog v-model="model">
    <q-card class="advanced-filter-card">
      <q-card-section
        ><div class="prts-h2">{{ t('editor.advancedFilters') }}</div></q-card-section
      >
      <q-card-section class="q-gutter-md">
        <q-input v-model="draft.query" outlined dense :label="t('editor.searchQuery')" />
        <q-select
          v-model="draft.scopeType"
          :options="scopeOptions"
          emit-value
          map-options
          outlined
          dense
          :label="t('editor.searchScope')"
        />
        <q-input
          v-if="draft.scopeType === 'path'"
          v-model="draft.path"
          outlined
          dense
          :label="t('editor.path')"
        />
        <q-select
          v-if="draft.scopeType === 'file'"
          v-model="draft.fileId"
          :options="fileOptions"
          emit-value
          map-options
          outlined
          dense
          :label="t('editor.file')"
        />
        <q-select
          v-if="draft.scopeType === 'current_file'"
          v-model="draft.currentFileId"
          :options="fileOptions"
          emit-value
          map-options
          outlined
          dense
          disable
          :label="t('editor.scopeCurrentFile')"
        />
        <q-input
          v-if="draft.scopeType === 'current_task'"
          v-model.number="draft.taskId"
          type="number"
          outlined
          dense
          :label="t('editor.taskId')"
        />
        <div v-for="(condition, index) in draft.conditions" :key="index" class="condition-row">
          <q-select
            v-model="condition.field"
            :options="fieldOptions"
            emit-value
            map-options
            dense
            outlined
          />
          <q-select
            v-model="condition.operator"
            :options="operatorOptions"
            emit-value
            map-options
            dense
            outlined
          />
          <q-input v-model="condition.value" dense outlined />
          <q-btn flat round dense icon="mdi-close" @click="draft.conditions.splice(index, 1)" />
        </div>
        <q-btn
          flat
          no-caps
          icon="mdi-plus"
          :label="t('editor.addCondition')"
          @click="addCondition"
        />
        <q-select
          v-model="draft.states"
          :options="stateOptions"
          emit-value
          map-options
          multiple
          outlined
          dense
          :label="t('editor.stateFilter')"
        />
        <div class="row q-gutter-md">
          <q-toggle
            v-model="draft.includeHidden"
            :disable="!canIncludeHidden"
            :label="t('editor.includeHidden')"
          />
          <q-toggle v-model="draft.vector" :label="t('editor.vectorSearch')" />
        </div>
      </q-card-section>
      <q-card-actions align="right">
        <q-btn v-close-popup flat no-caps :label="t('common.cancel')" />
        <q-btn
          unelevated
          no-caps
          color="primary"
          text-color="dark"
          :label="t('common.search')"
          :disable="!scopeReady"
          @click="submit"
        />
      </q-card-actions>
    </q-card>
  </q-dialog>
</template>

<style scoped>
.advanced-filter-card {
  width: 760px;
  max-width: 94vw;
}
.condition-row {
  display: grid;
  grid-template-columns: 1.2fr 1fr 1.4fr auto;
  gap: 8px;
}
@media (max-width: 700px) {
  .condition-row {
    grid-template-columns: 1fr;
  }
}
</style>
