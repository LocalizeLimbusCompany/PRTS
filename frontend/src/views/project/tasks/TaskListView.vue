<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useQuasar } from 'quasar'

import { apiErrorMessage, tasksApi, type TaskListItemDto } from '@/api'
import { hasProjectCapability } from '@/lib/capabilities'
import { taskProgressPercent } from '@/lib/projectTasks'
import { useProjectWorkspace } from '@/lib/projectWorkspace'

const { detail, projectId } = useProjectWorkspace()
const $q = useQuasar()
const { t } = useI18n()
const tasks = ref<TaskListItemDto[]>([])
const nextAfter = ref<number | null>(null)
const loading = ref(false)
const loaded = ref(false)
let loadRequest = 0
const canManage = computed(() => hasProjectCapability(detail.value?.capabilities, 'manage_tasks'))

async function load(reset = false) {
  if (loading.value && !reset) return
  const request = ++loadRequest
  const requestedProjectId = projectId.value
  if (reset) {
    tasks.value = []
    nextAfter.value = null
    loaded.value = false
  }
  loading.value = true
  try {
    const page = await tasksApi.list(requestedProjectId, {
      after: reset ? undefined : (nextAfter.value ?? undefined),
      limit: 24,
    })
    if (request !== loadRequest) return
    tasks.value = reset ? page.items : [...tasks.value, ...page.items]
    nextAfter.value = page.next_after
    loaded.value = true
  } catch (error) {
    if (request !== loadRequest) return
    $q.notify({ type: 'negative', message: apiErrorMessage(error, t('project.tasks.loadFailed')) })
  } finally {
    if (request === loadRequest) loading.value = false
  }
}

onMounted(() => load(true))
watch(projectId, () => load(true))
</script>

<template>
  <section class="task-list">
    <div class="task-section-heading">
      <div>
        <div class="prts-label">{{ $t('project.sections.tasks') }}</div>
        <h2>{{ $t('project.tasks.heading') }}</h2>
        <p>{{ $t('project.tasks.description') }}</p>
      </div>
      <q-btn
        v-if="canManage"
        unelevated
        no-caps
        color="primary"
        text-color="dark"
        icon="mdi-clipboard-plus-outline"
        :label="$t('project.tasks.create')"
        :to="{ name: 'project-task-new', params: { id: projectId } }"
      />
    </div>

    <q-skeleton v-if="!loaded && loading" height="240px" square />
    <div v-else-if="tasks.length === 0" class="prts-empty task-list__empty">
      <q-icon name="mdi-clipboard-text-outline" size="36px" />
      <strong>{{ $t('project.tasks.empty') }}</strong>
      <span>{{ $t('project.tasks.emptyHint') }}</span>
    </div>
    <div v-else class="task-list__grid">
      <router-link
        v-for="task in tasks"
        :key="task.id"
        class="task-card"
        :to="{ name: 'project-task-detail', params: { id: projectId, taskId: task.id } }"
      >
        <header>
          <span class="prts-mono">TASK-{{ task.id }}</span>
          <q-badge v-if="task.no_work_required" outline color="positive">
            {{ $t('project.tasks.noWork') }}
          </q-badge>
          <span v-else class="task-card__percent prts-mono">
            {{ taskProgressPercent(task.denominator, task.completed) }}%
          </span>
        </header>
        <h3>{{ task.title }}</h3>
        <div class="task-card__progress">
          <q-linear-progress :value="task.completion_ratio" color="primary" size="5px" />
          <span class="prts-mono">
            {{ $t('project.tasks.progressCount', { completed: task.completed, total: task.denominator }) }}
          </span>
        </div>
        <footer>
          <span><q-icon name="mdi-file-multiple-outline" /> {{ task.file_count }}</span>
          <span>{{ new Date(task.updated_at).toLocaleDateString() }}</span>
        </footer>
      </router-link>
    </div>
    <div v-if="nextAfter !== null" class="row justify-center q-mt-lg">
      <q-btn
        outline
        no-caps
        :loading="loading"
        :label="$t('project.tasks.loadMore')"
        @click="load()"
      />
    </div>
  </section>
</template>

<style scoped>
.task-list,
.task-section-heading,
.task-section-heading > div,
.task-card,
.task-card__progress {
  display: grid;
}

.task-list {
  gap: 16px;
}

.task-section-heading {
  grid-template-columns: minmax(0, 1fr) auto;
  align-items: end;
  gap: 20px;
}

.task-section-heading > div {
  gap: 4px;
}

.task-section-heading h2,
.task-card h3,
.task-section-heading p {
  margin: 0;
}

.task-section-heading h2 {
  color: var(--prts-text-strong);
  font: 500 22px var(--font-display);
}

.task-section-heading p {
  color: var(--prts-text-dim);
}

.task-list__grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(280px, 1fr));
  gap: 12px;
}

.task-card {
  gap: 14px;
  min-height: 190px;
  padding: 16px;
  border: 1px solid var(--prts-border);
  background: var(--prts-panel);
  color: var(--prts-text);
}

.task-card:hover {
  border-color: var(--prts-accent);
  color: var(--prts-text);
}

.task-card header,
.task-card footer {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  color: var(--prts-text-dim);
  font-size: 11px;
}

.task-card h3 {
  color: var(--prts-text-strong);
  font: 600 17px var(--font-display);
}

.task-card__percent {
  color: var(--prts-accent);
}

.task-card__progress {
  gap: 8px;
  align-self: end;
  font-size: 11px;
  color: var(--prts-text-dim);
}

.task-list__empty {
  display: grid;
  justify-items: center;
  gap: 8px;
  border: 1px dashed var(--prts-border);
}

@media (max-width: 640px) {
  .task-section-heading {
    grid-template-columns: 1fr;
  }
}
</style>
