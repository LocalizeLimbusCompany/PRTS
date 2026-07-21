<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useQuasar } from 'quasar'

import { apiErrorMessage, tasksApi, type TaskDetailDto, type TaskFileDto } from '@/api'
import MarkdownView from '@/components/MarkdownView.vue'
import { hasProjectCapability } from '@/lib/capabilities'
import { taskProgressPercent } from '@/lib/projectTasks'
import { useProjectWorkspace } from '@/lib/projectWorkspace'

const props = defineProps<{ taskId: number }>()
const { detail, projectId } = useProjectWorkspace()
const $q = useQuasar()
const { t } = useI18n()
const task = ref<TaskDetailDto | null>(null)
const fileQuery = ref('')
const deferredFileQuery = ref('')
const loading = ref(true)
let loadRequest = 0
let fileQueryTimer: ReturnType<typeof setTimeout> | null = null
const canManage = computed(() => hasProjectCapability(detail.value?.capabilities, 'manage_tasks'))
const canTranslate = computed(() => hasProjectCapability(detail.value?.capabilities, 'edit_entry'))
const liveFiles = computed(
  () => task.value?.files.filter((file) => file.live_file_id !== null) ?? [],
)
const progressPercent = computed(() =>
  task.value ? taskProgressPercent(task.value.denominator, task.value.completed) : 0,
)
const fileSearchIndex = computed(
  () =>
    new Map(
      (task.value?.files ?? []).map((file) => [
        file.id,
        `${file.path ?? ''}\u0000${file.name ?? ''}\u0000${file.file_id_snapshot}`.toLocaleLowerCase(),
      ]),
    ),
)
const visibleFiles = computed(() => {
  const normalized = deferredFileQuery.value.trim().toLocaleLowerCase()
  const files = task.value?.files ?? []
  if (!normalized) return files
  return files.filter((file) => fileSearchIndex.value.get(file.id)?.includes(normalized))
})

// Defer large task-file scans until typing pauses while clearing results immediately.
watch(fileQuery, (value) => {
  if (fileQueryTimer) clearTimeout(fileQueryTimer)
  if (!value) {
    deferredFileQuery.value = ''
    fileQueryTimer = null
    return
  }
  fileQueryTimer = setTimeout(() => {
    deferredFileQuery.value = String(value)
    fileQueryTimer = null
  }, 120)
})

async function load() {
  const request = ++loadRequest
  const requestedProjectId = projectId.value
  const requestedTaskId = props.taskId
  if (fileQueryTimer) clearTimeout(fileQueryTimer)
  fileQueryTimer = null
  fileQuery.value = ''
  deferredFileQuery.value = ''
  task.value = null
  loading.value = true
  try {
    const loadedTask = await tasksApi.get(requestedProjectId, requestedTaskId)
    if (request !== loadRequest) return
    task.value = loadedTask
  } catch (error) {
    if (request !== loadRequest) return
    $q.notify({ type: 'negative', message: apiErrorMessage(error, t('project.tasks.loadFailed')) })
  } finally {
    if (request === loadRequest) loading.value = false
  }
}

onMounted(load)
watch([projectId, () => props.taskId], load)
onBeforeUnmount(() => {
  if (fileQueryTimer) clearTimeout(fileQueryTimer)
})
</script>

<template>
  <section class="task-detail">
    <q-skeleton v-if="loading" height="300px" square />
    <template v-else-if="task">
      <header class="task-detail__header">
        <div>
          <router-link
            :to="{ name: 'project-tasks', params: { id: projectId } }"
            class="prts-label"
          >
            <q-icon name="mdi-arrow-left" /> {{ $t('project.tasks.back') }}
          </router-link>
          <h2>{{ task.title }}</h2>
          <span class="prts-mono prts-dim">TASK-{{ task.id }}</span>
        </div>
        <div class="row q-gutter-sm">
          <q-btn
            v-if="canManage"
            outline
            no-caps
            icon="mdi-pencil-outline"
            :label="$t('project.tasks.manage')"
            :to="{ name: 'project-task-manage', params: { id: projectId, taskId: task.id } }"
          />
          <q-btn
            v-if="canTranslate"
            unelevated
            no-caps
            color="primary"
            text-color="dark"
            icon="mdi-translate"
            :disable="liveFiles.length === 0"
            :label="$t('project.tasks.translateHere')"
            :to="{ name: 'editor', params: { id: projectId }, query: { task: task.id } }"
          />
        </div>
      </header>

      <q-card flat bordered class="task-detail__progress">
        <q-card-section>
          <div class="task-progress__summary">
            <div>
              <span class="prts-label">{{ $t('project.progress') }}</span>
              <strong class="prts-mono">{{ progressPercent }}%</strong>
            </div>
            <q-badge v-if="task.no_work_required" outline color="positive">
              {{ $t('project.tasks.noWork') }}
            </q-badge>
            <span v-else class="prts-mono prts-dim">
              {{
                $t('project.tasks.progressCount', {
                  completed: task.completed,
                  total: task.denominator,
                })
              }}
            </span>
          </div>
          <q-linear-progress :value="task.completion_ratio" color="primary" size="7px" />
        </q-card-section>
      </q-card>

      <q-card flat bordered>
        <q-card-section>
          <div class="prts-label q-mb-md">{{ $t('project.tasks.introduction') }}</div>
          <MarkdownView v-if="task.description" :source="task.description" />
          <div v-else class="prts-dim">{{ $t('project.tasks.noDescription') }}</div>
        </q-card-section>
      </q-card>

      <q-card flat bordered class="task-detail__files">
        <q-card-section>
          <div class="task-files__heading">
            <div>
              <div class="prts-label">{{ $t('project.tasks.files') }}</div>
              <span class="prts-mono prts-dim">
                {{ $t('project.tasks.fileCount', { count: task.files.length }) }}
              </span>
            </div>
            <q-input
              v-if="task.files.length > 0"
              v-model="fileQuery"
              dense
              outlined
              clearable
              debounce="0"
              :placeholder="$t('project.tasks.searchFiles')"
            >
              <template #prepend><q-icon name="mdi-magnify" /></template>
            </q-input>
          </div>
          <div v-if="task.files.length === 0" class="prts-dim">
            {{ $t('project.tasks.noFiles') }}
          </div>
          <div v-else-if="visibleFiles.length === 0" class="prts-empty">
            {{ $t('project.files.empty') }}
          </div>
          <q-virtual-scroll
            v-else
            class="task-files"
            :items="visibleFiles"
            :virtual-scroll-item-size="42"
            :virtual-scroll-slice-size="36"
            virtual-scroll-item-key="id"
          >
            <template #default="{ item: file }: { item: TaskFileDto }">
              <div class="task-files__row">
                <router-link
                  v-if="file.live_file_id !== null"
                  :to="{
                    name: 'editor',
                    params: { id: projectId },
                    query: { file: file.live_file_id },
                  }"
                >
                  <q-icon name="mdi-file-document-outline" />
                  <span>{{ file.path }}</span>
                </router-link>
                <div v-else class="task-files__missing">
                  <q-icon name="mdi-file-remove-outline" />
                  <span>{{
                    $t('project.tasks.unavailableFile', { id: file.file_id_snapshot })
                  }}</span>
                </div>
              </div>
            </template>
          </q-virtual-scroll>
        </q-card-section>
      </q-card>
    </template>
  </section>
</template>

<style scoped>
.task-detail,
.task-detail__header > div:first-child,
.task-files {
  display: grid;
}

.task-detail {
  gap: 18px;
}

.task-detail__header,
.task-progress__summary,
.task-files a,
.task-files__missing {
  display: flex;
  align-items: center;
}

.task-detail__header,
.task-progress__summary {
  justify-content: space-between;
  gap: 20px;
}

.task-detail__header > div:first-child {
  gap: 5px;
}

.task-detail__header h2 {
  margin: 0;
  color: var(--prts-text-strong);
  font: 600 24px var(--font-display);
}

.task-detail__progress {
  border-top-color: var(--prts-accent);
}

.task-progress__summary {
  margin-bottom: 12px;
}

.task-progress__summary > div {
  display: flex;
  align-items: baseline;
  gap: 12px;
}

.task-progress__summary strong {
  color: var(--prts-accent);
  font-size: 26px;
}

.task-detail__files {
  min-width: 0;
}

.task-files {
  max-height: min(52vh, 560px);
  overflow: auto;
  overscroll-behavior: contain;
  border: 1px solid var(--prts-border-soft);
}

.task-files__heading,
.task-files__heading > div {
  display: flex;
}

.task-files__heading {
  align-items: end;
  justify-content: space-between;
  gap: 16px;
  margin-bottom: 12px;
}

.task-files__heading > div {
  align-items: baseline;
  gap: 10px;
}

.task-files__heading .q-input {
  width: min(420px, 48%);
}

.task-files__row {
  min-height: 42px;
  border-bottom: 1px solid var(--prts-border-soft);
}

.task-files a,
.task-files__missing {
  gap: 9px;
  min-height: 42px;
  padding: 0 11px;
  color: var(--prts-text);
}

.task-files a {
  width: 100%;
}

.task-files a span,
.task-files__missing span {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.task-files a:hover {
  background: var(--prts-panel-2);
  color: var(--prts-accent);
}

.task-files__missing {
  color: var(--prts-text-faint);
}

@media (max-width: 880px) {
  .task-detail__header {
    align-items: flex-start;
    flex-direction: column;
  }

  .task-files__heading {
    align-items: stretch;
    flex-direction: column;
  }

  .task-files__heading .q-input {
    width: 100%;
  }
}
</style>
