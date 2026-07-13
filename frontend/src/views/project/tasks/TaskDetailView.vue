<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useQuasar } from 'quasar'

import { apiErrorMessage, tasksApi, type TaskDetailDto } from '@/api'
import MarkdownView from '@/components/MarkdownView.vue'
import { hasProjectCapability } from '@/lib/capabilities'
import { taskProgressPercent } from '@/lib/projectTasks'
import { useProjectWorkspace } from '@/lib/projectWorkspace'

const props = defineProps<{ taskId: number }>()
const { detail, projectId } = useProjectWorkspace()
const $q = useQuasar()
const { t } = useI18n()
const task = ref<TaskDetailDto | null>(null)
const loading = ref(true)
let loadRequest = 0
const canManage = computed(() => hasProjectCapability(detail.value?.capabilities, 'manage_tasks'))
const canTranslate = computed(() => hasProjectCapability(detail.value?.capabilities, 'edit_entry'))
const liveFiles = computed(() => task.value?.files.filter((file) => file.live_file_id !== null) ?? [])
const progressPercent = computed(() =>
  task.value ? taskProgressPercent(task.value.denominator, task.value.completed) : 0,
)

async function load() {
  const request = ++loadRequest
  const requestedProjectId = projectId.value
  const requestedTaskId = props.taskId
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
</script>

<template>
  <section class="task-detail">
    <q-skeleton v-if="loading" height="300px" square />
    <template v-else-if="task">
      <header class="task-detail__header">
        <div>
          <router-link :to="{ name: 'project-tasks', params: { id: projectId } }" class="prts-label">
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
              {{ $t('project.tasks.progressCount', { completed: task.completed, total: task.denominator }) }}
            </span>
          </div>
          <q-linear-progress :value="task.completion_ratio" color="primary" size="7px" />
        </q-card-section>
      </q-card>

      <div class="task-detail__grid">
        <q-card flat bordered>
          <q-card-section>
            <div class="prts-label q-mb-md">{{ $t('project.tasks.introduction') }}</div>
            <MarkdownView v-if="task.description" :source="task.description" />
            <div v-else class="prts-dim">{{ $t('project.tasks.noDescription') }}</div>
          </q-card-section>
        </q-card>
        <q-card flat bordered>
          <q-card-section>
            <div class="prts-label q-mb-md">{{ $t('project.tasks.files') }}</div>
            <div v-if="task.files.length === 0" class="prts-dim">
              {{ $t('project.tasks.noFiles') }}
            </div>
            <div v-else class="task-files">
              <template v-for="file in task.files" :key="file.id">
                <router-link
                  v-if="file.live_file_id !== null"
                  :to="{ name: 'editor', params: { id: projectId }, query: { file: file.live_file_id } }"
                >
                  <q-icon name="mdi-file-document-outline" />
                  <span>{{ file.path }}</span>
                </router-link>
                <div v-else class="task-files__missing">
                  <q-icon name="mdi-file-remove-outline" />
                  <span>{{ $t('project.tasks.unavailableFile', { id: file.file_id_snapshot }) }}</span>
                </div>
              </template>
            </div>
          </q-card-section>
        </q-card>
      </div>
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

.task-detail__grid {
  display: grid;
  grid-template-columns: minmax(0, 1fr) minmax(280px, 0.42fr);
  gap: 16px;
}

.task-files {
  gap: 1px;
  border: 1px solid var(--prts-border-soft);
}

.task-files a,
.task-files__missing {
  gap: 9px;
  min-height: 42px;
  padding: 0 11px;
  border-bottom: 1px solid var(--prts-border-soft);
  color: var(--prts-text);
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

  .task-detail__grid {
    grid-template-columns: 1fr;
  }
}
</style>
