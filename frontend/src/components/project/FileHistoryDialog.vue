<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useQuasar } from 'quasar'
import { useI18n } from 'vue-i18n'

import { apiErrorMessage, fileHistoryApi, projectsApi, type FileChangeSetDto } from '@/api'
import {
  canRestoreFileChangeSet,
  fileHistoryChangedFields,
  fileHistoryTarget,
  type FileHistoryTarget,
} from '@/lib/fileHistory'

const props = defineProps<{
  modelValue: boolean
  projectId: number
  target?: FileHistoryTarget | null
  canRollback: boolean
}>()

const emit = defineEmits<{
  'update:modelValue': [value: boolean]
  changed: []
}>()

const $q = useQuasar()
const { t, te } = useI18n()
const records = ref<FileChangeSetDto[]>([])
const nextAfter = ref<string | null>(null)
const loading = ref(false)
const mutatingId = ref<string | null>(null)

const open = computed({
  get: () => props.modelValue,
  set: (value) => emit('update:modelValue', value),
})
const latestChangeByTarget = computed(() => {
  const latest = new Map<string, string>()
  for (const changeSet of records.value) {
    const target = fileHistoryTarget(changeSet)
    const key = target ? `${target.kind}-${target.id}` : null
    if (key && !latest.has(key)) latest.set(key, changeSet.id)
  }
  return latest
})

function operationLabel(operation: string): string {
  const key = `project.history.operations.${operation}`
  return te(key) ? t(key) : operation
}

function entityLabel(entity: string): string {
  const key = `project.history.entities.${entity}`
  return te(key) ? t(key) : entity
}

function fieldLabel(field: string): string {
  const key = `project.history.fields.${field}`
  return te(key) ? t(key) : field
}

function query(after?: string) {
  return {
    ...(after ? { after } : {}),
    ...(props.target?.kind === 'file' ? { file_id: props.target.id } : {}),
    ...(props.target?.kind === 'folder' ? { folder_id: props.target.id } : {}),
    limit: 30,
  }
}

/** Old delete records remain immutable; only the latest operation for a live target may restore. */
function canRestore(changeSet: FileChangeSetDto): boolean {
  const target = fileHistoryTarget(changeSet)
  return Boolean(
    target &&
    canRestoreFileChangeSet(changeSet) &&
    latestChangeByTarget.value.get(`${target.kind}-${target.id}`) === changeSet.id,
  )
}

/** Read one keyset page; filtering remains server-side and never uses large offsets. */
async function load(reset = true) {
  loading.value = true
  try {
    const page = await fileHistoryApi.list(
      props.projectId,
      query(reset ? undefined : (nextAfter.value ?? undefined)),
    )
    records.value = reset ? page.items : [...records.value, ...page.items]
    nextAfter.value = page.next_after
  } catch (error) {
    $q.notify({ type: 'negative', message: apiErrorMessage(error) })
  } finally {
    loading.value = false
  }
}

/** Restore binds exactly to the deletion change-set that owns the soft-deleted rows. */
function restore(changeSet: FileChangeSetDto) {
  const target = fileHistoryTarget(changeSet)
  if (!target) return
  $q.dialog({
    title: t('project.history.restore'),
    message: t('project.history.restoreConfirm', { path: changeSet.path_snapshot }),
    cancel: true,
    ok: { label: t('project.history.restore'), color: 'primary', noCaps: true },
  }).onOk(async () => {
    mutatingId.value = changeSet.id
    try {
      if (target.kind === 'file') {
        await projectsApi.restoreFile(props.projectId, target.id, changeSet.id)
      } else {
        await projectsApi.restoreFolder(props.projectId, target.id, changeSet.id)
      }
      emit('changed')
      await load()
    } catch (error) {
      $q.notify({ type: 'negative', message: apiErrorMessage(error) })
    } finally {
      mutatingId.value = null
    }
  })
}

/** Ask the server to materialize the target and append a new current-to-target change set. */
function rollback(changeSet: FileChangeSetDto) {
  const target = fileHistoryTarget(changeSet)
  if (!target) return
  $q.dialog({
    title: t('project.history.rollback'),
    message: t('project.history.rollbackConfirm', { path: changeSet.path_snapshot }),
    cancel: true,
    ok: { label: t('project.history.rollback'), color: 'warning', noCaps: true },
  }).onOk(async () => {
    mutatingId.value = changeSet.id
    try {
      if (target.kind === 'file') {
        await fileHistoryApi.rollbackFile(props.projectId, target.id, changeSet.id)
      } else {
        await fileHistoryApi.rollbackFolder(props.projectId, target.id, changeSet.id)
      }
      emit('changed')
      await load()
    } catch (error) {
      $q.notify({ type: 'negative', message: apiErrorMessage(error) })
    } finally {
      mutatingId.value = null
    }
  })
}

watch(
  () => [props.modelValue, props.projectId, props.target?.kind, props.target?.id],
  ([visible]) => {
    if (visible) void load()
  },
)
</script>

<template>
  <q-dialog v-model="open">
    <q-card class="history-dialog">
      <q-card-section class="history-dialog__head">
        <div>
          <div class="prts-label">{{ $t('project.history.label') }}</div>
          <div class="prts-h2">{{ $t('project.history.heading') }}</div>
        </div>
        <q-btn flat round dense icon="mdi-close" @click="open = false" />
      </q-card-section>
      <q-separator />

      <q-card-section class="history-dialog__body">
        <q-inner-loading :showing="loading && records.length === 0" />
        <div v-if="!loading && records.length === 0" class="prts-empty">
          {{ $t('project.history.empty') }}
        </div>
        <q-timeline v-else color="primary" layout="dense">
          <q-timeline-entry
            v-for="changeSet in records"
            :key="changeSet.id"
            :title="operationLabel(changeSet.operation)"
            :subtitle="new Date(changeSet.created_at).toLocaleString()"
            :icon="changeSet.operation === 'delete' ? 'mdi-delete-clock-outline' : 'mdi-history'"
          >
            <div class="history-dialog__entry">
              <strong class="prts-mono">{{ changeSet.path_snapshot }}</strong>
              <span class="prts-mono prts-dim">{{ changeSet.id }}</span>
              <q-list dense bordered separator>
                <q-item v-for="item in changeSet.items" :key="item.id">
                  <q-item-section>
                    <q-item-label>
                      {{ entityLabel(item.entity_type) }} #{{ item.entity_id ?? '—' }} ·
                      {{ operationLabel(item.operation) }}
                    </q-item-label>
                    <q-item-label caption>
                      {{
                        fileHistoryChangedFields(item).map(fieldLabel).join(' · ') ||
                        $t('project.history.noFieldChange')
                      }}
                    </q-item-label>
                  </q-item-section>
                </q-item>
              </q-list>
              <div v-if="canRollback" class="row q-gutter-sm">
                <q-btn
                  v-if="canRestore(changeSet)"
                  outline
                  dense
                  no-caps
                  color="primary"
                  icon="mdi-restore"
                  :label="$t('project.history.restore')"
                  :loading="mutatingId === changeSet.id"
                  @click="restore(changeSet)"
                />
                <q-btn
                  v-else-if="changeSet.operation !== 'delete' && fileHistoryTarget(changeSet)"
                  flat
                  dense
                  no-caps
                  color="warning"
                  icon="mdi-backup-restore"
                  :label="$t('project.history.rollback')"
                  :loading="mutatingId === changeSet.id"
                  @click="rollback(changeSet)"
                />
              </div>
            </div>
          </q-timeline-entry>
        </q-timeline>
        <q-btn
          v-if="nextAfter"
          outline
          no-caps
          class="full-width"
          :label="$t('project.history.loadMore')"
          :loading="loading"
          @click="load(false)"
        />
      </q-card-section>
    </q-card>
  </q-dialog>
</template>

<style scoped>
.history-dialog {
  width: min(920px, 94vw);
  max-width: 94vw;
}

.history-dialog__head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
}

.history-dialog__body {
  position: relative;
  max-height: 76vh;
  overflow: auto;
}

.history-dialog__entry {
  display: grid;
  gap: 10px;
}
</style>
