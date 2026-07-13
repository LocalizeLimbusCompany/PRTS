<script setup lang="ts">
import { computed, onBeforeUnmount, ref } from 'vue'
import { useQuasar } from 'quasar'
import { useI18n } from 'vue-i18n'

import { apiErrorMessage } from '@/api'
import type { UploadBatchFileDto } from '@/api/uploads'
import { UPLOAD_BATCH_TERMINAL_STATES, useUploadBatch } from '@/composables/useUploadBatch'

const props = defineProps<{
  modelValue: boolean
  projectId: number
}>()

const emit = defineEmits<{
  'update:modelValue': [value: boolean]
  completed: []
}>()

const $q = useQuasar()
const { t, te } = useI18n()
const pickedFiles = ref<File[]>([])
const upload = useUploadBatch(() => props.projectId)
const pollTimer = ref<number | null>(null)

const open = computed({
  get: () => props.modelValue,
  set: (value) => emit('update:modelValue', value),
})
const active = computed(
  () => Boolean(upload.batch.value) && !UPLOAD_BATCH_TERMINAL_STATES.has(upload.batch.value!.state),
)
const canCancel = computed(
  () =>
    upload.batch.value &&
    ['uploading', 'queued', 'processing', 'cancelling'].includes(upload.batch.value.state),
)
const progress = computed(() =>
  upload.totalBytes.value > 0 ? upload.totalLoaded.value / upload.totalBytes.value : 0,
)

/** Keep file/folder selection limited to raw JSON files without reading their bytes. */
function jsonFileFilter(files: readonly File[] | FileList) {
  return Array.from(files).filter((file) => file.name.toLocaleLowerCase().endsWith('.json'))
}

/** Format sizes locally without copying the underlying File into application state. */
function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`
}

/** Resolve stable backend state/error codes through the bilingual allowlist. */
function codeLabel(prefix: string, code: string | null): string {
  if (!code) return ''
  const key = `${prefix}.${code}`
  return te(key) ? t(key) : code
}

/** Render only allowlisted parser positions returned by the server. */
function errorPosition(file: UploadBatchFileDto): string {
  const details = file.error_details
  if (!details) return ''
  const values = ['ordinal', 'first_ordinal', 'duplicate_ordinal', 'line', 'column']
    .filter((key) => typeof details[key] === 'number')
    .map((key) => t(`project.upload.positions.${key}`, { value: details[key] }))
  return values.join(' · ')
}

/** Prefer the browser transfer state until the next durable server snapshot arrives. */
function fileState(file: UploadBatchFileDto): string {
  const local = upload.queue.value[file.ordinal]
  if (local?.state === 'uploading') return 'uploading'
  return file.state
}

/** Avoid retrying while sibling files are still queued or processing in the same batch. */
function retryAvailable(file: UploadBatchFileDto): boolean {
  const snapshot = upload.batch.value
  if (!snapshot || !['failed', 'cancelled', 'expired'].includes(file.state)) return false
  if (['cancelling', 'cancelled', 'expired', 'succeeded'].includes(snapshot.state)) return false
  return snapshot.files.every((item) =>
    ['failed', 'cancelled', 'expired', 'succeeded'].includes(item.state),
  )
}

function stopPolling() {
  if (pollTimer.value !== null) window.clearTimeout(pollTimer.value)
  pollTimer.value = null
}

/** Poll durable processing/cancellation state until the batch reaches a terminal state. */
async function poll() {
  stopPolling()
  if (!upload.batch.value || !open.value) return
  try {
    const snapshot = await upload.refresh()
    if (!snapshot || UPLOAD_BATCH_TERMINAL_STATES.has(snapshot.state)) {
      emit('completed')
      return
    }
  } catch (error) {
    $q.notify({ type: 'negative', message: apiErrorMessage(error) })
    return
  }
  pollTimer.value = window.setTimeout(() => void poll(), 1500)
}

/** Declare and stream the selected raw files using server-provided concurrency. */
async function start() {
  if (pickedFiles.value.length === 0) return
  try {
    await upload.start(pickedFiles.value)
    await poll()
  } catch (error) {
    $q.notify({ type: 'negative', message: apiErrorMessage(error) })
  }
}

/** Create and upload a fresh byte-zero attempt for one failed logical file. */
async function retry(file: UploadBatchFileDto) {
  try {
    await upload.retry(file)
    await poll()
  } catch (error) {
    $q.notify({ type: 'negative', message: apiErrorMessage(error) })
  }
}

/** Request durable cancellation; in-flight file transactions may still finish atomically. */
async function cancel() {
  try {
    await upload.cancel()
    await poll()
  } catch (error) {
    $q.notify({ type: 'negative', message: apiErrorMessage(error) })
  }
}

/** Release a terminal snapshot and retain no file bytes beyond this dialog instance. */
function newBatch() {
  pickedFiles.value = []
  upload.reset()
}

/** A hidden dialog stops polling; reopening a non-terminal batch resumes it. */
function visibilityChanged(value: boolean) {
  if (value && active.value) void poll()
  if (!value) {
    stopPolling()
    if (!active.value) newBatch()
  }
}

onBeforeUnmount(() => {
  stopPolling()
})
</script>

<template>
  <q-dialog v-model="open" :persistent="Boolean(active)" @update:model-value="visibilityChanged">
    <q-card class="upload-dialog">
      <q-card-section class="upload-dialog__head">
        <div>
          <div class="prts-label">{{ $t('project.upload.label') }}</div>
          <div class="prts-h2">{{ $t('project.upload.heading') }}</div>
        </div>
        <q-badge
          v-if="upload.batch.value"
          outline
          :color="UPLOAD_BATCH_TERMINAL_STATES.has(upload.batch.value.state) ? 'grey' : 'primary'"
          :label="codeLabel('project.upload.states', upload.batch.value.state)"
        />
      </q-card-section>

      <q-separator />
      <q-card-section v-if="!upload.batch.value" class="upload-dialog__selection">
        <div class="prts-dim">{{ $t('project.upload.description') }}</div>
        <div class="upload-dialog__pickers">
          <q-file
            v-model="pickedFiles"
            outlined
            multiple
            accept=".json,application/json"
            :filter="jsonFileFilter"
            :label="$t('project.upload.chooseFiles')"
          >
            <template #prepend><q-icon name="mdi-file-upload-outline" /></template>
          </q-file>
          <q-file
            v-model="pickedFiles"
            outlined
            multiple
            webkitdirectory
            accept=".json,application/json"
            :filter="jsonFileFilter"
            :label="$t('project.upload.chooseFolder')"
          >
            <template #prepend><q-icon name="mdi-folder-upload-outline" /></template>
          </q-file>
        </div>
        <q-list v-if="pickedFiles.length" bordered separator>
          <q-item v-for="file in pickedFiles" :key="file.webkitRelativePath || file.name">
            <q-item-section>
              <q-item-label class="prts-mono">
                {{ file.webkitRelativePath || file.name }}
              </q-item-label>
            </q-item-section>
            <q-item-section side>{{ formatBytes(file.size) }}</q-item-section>
          </q-item>
        </q-list>
      </q-card-section>

      <q-card-section v-else class="upload-dialog__batch">
        <q-linear-progress
          v-if="upload.running.value && upload.totalBytes.value > 0"
          :value="progress"
          size="6px"
          color="primary"
        />
        <q-banner
          v-if="upload.batch.value.state === 'cancelling'"
          dense
          class="bg-warning text-dark"
        >
          {{ $t('project.upload.cancellingHint') }}
        </q-banner>
        <div class="upload-dialog__summary prts-mono prts-dim">
          <span>#{{ upload.batch.value.id }}</span>
          <span>{{ upload.batch.value.declared_file_count }} {{ $t('project.files.count') }}</span>
          <span>{{ formatBytes(upload.batch.value.declared_total_bytes) }}</span>
        </div>

        <q-list bordered separator>
          <q-expansion-item
            v-for="file in upload.batch.value.files"
            :key="file.id"
            group="upload-files"
            expand-separator
          >
            <template #header>
              <q-item-section avatar>
                <q-icon name="mdi-file-document-outline" color="primary" />
              </q-item-section>
              <q-item-section>
                <q-item-label class="prts-mono">{{ file.path }}</q-item-label>
                <q-item-label v-if="file.last_error_code" caption class="text-negative">
                  {{ codeLabel('project.upload.errors', file.last_error_code) }}
                  <span v-if="errorPosition(file)"> · {{ errorPosition(file) }}</span>
                </q-item-label>
              </q-item-section>
              <q-item-section side>
                <div class="row items-center q-gutter-sm">
                  <q-badge outline :label="codeLabel('project.upload.states', fileState(file))" />
                  <q-btn
                    v-if="['failed', 'cancelled', 'expired'].includes(file.state)"
                    flat
                    dense
                    no-caps
                    color="primary"
                    icon="mdi-refresh"
                    :label="$t('project.upload.retry')"
                    :disable="
                      !retryAvailable(file) || upload.running.value || upload.cancelling.value
                    "
                    @click.stop="retry(file)"
                  />
                </div>
              </q-item-section>
            </template>
            <q-list dense separator class="upload-dialog__attempts">
              <q-item v-for="attempt in file.attempts" :key="attempt.id">
                <q-item-section>
                  <q-item-label>
                    {{ $t('project.upload.attempt', { number: attempt.attempt_number }) }}
                  </q-item-label>
                  <q-item-label caption>
                    {{ new Date(attempt.started_at).toLocaleString() }}
                  </q-item-label>
                </q-item-section>
                <q-item-section side>
                  <span>{{ codeLabel('project.upload.states', attempt.state) }}</span>
                  <small class="prts-mono prts-dim">
                    {{ formatBytes(attempt.bytes_received) }} /
                    {{ formatBytes(file.declared_bytes) }}
                  </small>
                </q-item-section>
              </q-item>
            </q-list>
          </q-expansion-item>
        </q-list>
      </q-card-section>

      <q-separator />
      <q-card-actions align="right">
        <q-btn
          v-if="canCancel"
          flat
          no-caps
          color="negative"
          icon="mdi-cancel"
          :label="$t('project.upload.cancelBatch')"
          :loading="upload.cancelling.value"
          @click="cancel"
        />
        <q-space />
        <q-btn
          flat
          no-caps
          :disable="Boolean(active)"
          :label="$t('project.cancel')"
          @click="open = false"
        />
        <q-btn
          v-if="upload.batch.value && UPLOAD_BATCH_TERMINAL_STATES.has(upload.batch.value.state)"
          outline
          no-caps
          icon="mdi-plus"
          :label="$t('project.upload.newBatch')"
          @click="newBatch"
        />
        <q-btn
          v-if="!upload.batch.value"
          unelevated
          no-caps
          color="primary"
          text-color="dark"
          icon="mdi-upload"
          :label="$t('project.upload.start')"
          :disable="pickedFiles.length === 0"
          :loading="upload.running.value"
          @click="start"
        />
      </q-card-actions>
    </q-card>
  </q-dialog>
</template>

<style scoped>
.upload-dialog {
  width: min(880px, 94vw);
  max-width: 94vw;
}

.upload-dialog__head,
.upload-dialog__summary {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
}

.upload-dialog__selection,
.upload-dialog__batch {
  display: grid;
  max-height: 68vh;
  gap: 14px;
  overflow: auto;
}

.upload-dialog__pickers {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 10px;
}

.upload-dialog__attempts {
  background: var(--prts-panel-2);
}

@media (max-width: 680px) {
  .upload-dialog__pickers,
  .upload-dialog__summary {
    grid-template-columns: 1fr;
  }

  .upload-dialog__summary {
    display: grid;
  }
}
</style>
