<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRouter } from 'vue-router'
import { useQuasar } from 'quasar'

import { apiErrorMessage, projectsApi, tasksApi, type FileDto, type FolderDto } from '@/api'
import MarkdownEditor from '@/components/MarkdownEditor.vue'
import ProjectFileBrowser from '@/components/project/ProjectFileBrowser.vue'
import { hasProjectCapability } from '@/lib/capabilities'
import { useProjectWorkspace } from '@/lib/projectWorkspace'

const props = defineProps<{ taskId?: number }>()
const { detail, projectId } = useProjectWorkspace()
const router = useRouter()
const $q = useQuasar()
const { t } = useI18n()
const title = ref('')
const description = ref('')
const selectedFileIds = ref<number[]>([])
const folders = ref<FolderDto[]>([])
const files = ref<FileDto[]>([])
const loading = ref(true)
const saving = ref(false)
const deleting = ref(false)
let loadRequest = 0
const isEdit = computed(() => props.taskId !== undefined)
const canManage = computed(() => hasProjectCapability(detail.value?.capabilities, 'manage_tasks'))
const valid = computed(() => title.value.trim().length > 0 && title.value.trim().length <= 200)

async function load() {
  const request = ++loadRequest
  const requestedProjectId = projectId.value
  const requestedTaskId = props.taskId
  title.value = ''
  description.value = ''
  selectedFileIds.value = []
  loading.value = true
  try {
    const [tree, task] = await Promise.all([
      projectsApi.tree(requestedProjectId),
      requestedTaskId === undefined
        ? Promise.resolve(null)
        : tasksApi.get(requestedProjectId, requestedTaskId),
    ])
    if (request !== loadRequest) return
    folders.value = tree.folders
    files.value = tree.files
    if (task) {
      title.value = task.title
      description.value = task.description
      selectedFileIds.value = task.files
        .flatMap((file) => (file.live_file_id === null ? [] : [file.live_file_id]))
        .sort((left, right) => left - right)
    }
  } catch (error) {
    if (request !== loadRequest) return
    $q.notify({ type: 'negative', message: apiErrorMessage(error, t('project.tasks.loadFailed')) })
    await router.replace({ name: 'project-tasks', params: { id: requestedProjectId } })
  } finally {
    if (request === loadRequest) loading.value = false
  }
}

async function save() {
  if (!canManage.value || !valid.value) return
  saving.value = true
  try {
    const body = {
      title: title.value.trim(),
      description: description.value,
      file_ids: selectedFileIds.value,
    }
    const task = props.taskId === undefined
      ? await tasksApi.create(projectId.value, body)
      : await tasksApi.update(projectId.value, props.taskId, body)
    $q.notify({ type: 'positive', message: t('project.tasks.saved') })
    await router.replace({
      name: 'project-task-detail',
      params: { id: projectId.value, taskId: task.id },
    })
  } catch (error) {
    $q.notify({ type: 'negative', message: apiErrorMessage(error, t('project.tasks.saveFailed')) })
  } finally {
    saving.value = false
  }
}

function remove() {
  if (props.taskId === undefined || !canManage.value) return
  $q.dialog({
    title: t('project.tasks.delete'),
    message: t('project.tasks.deleteConfirm', { title: title.value }),
    cancel: true,
    ok: { label: t('project.tasks.delete'), color: 'negative', noCaps: true },
  }).onOk(async () => {
    deleting.value = true
    try {
      await tasksApi.remove(projectId.value, props.taskId!)
      $q.notify({ type: 'positive', message: t('project.tasks.deleted') })
      await router.replace({ name: 'project-tasks', params: { id: projectId.value } })
    } catch (error) {
      $q.notify({ type: 'negative', message: apiErrorMessage(error) })
    } finally {
      deleting.value = false
    }
  })
}

onMounted(load)
watch([projectId, () => props.taskId], load)
</script>

<template>
  <section class="task-manage">
    <q-inner-loading :showing="loading" />
    <template v-if="!loading && canManage">
      <header class="task-manage__header">
        <div>
          <div class="prts-label">{{ $t('project.sections.tasks') }}</div>
          <h2>{{ $t(isEdit ? 'project.tasks.editHeading' : 'project.tasks.createHeading') }}</h2>
          <p>{{ $t('project.tasks.manageHint') }}</p>
        </div>
        <q-btn
          v-if="isEdit"
          flat
          no-caps
          color="negative"
          icon="mdi-delete-outline"
          :loading="deleting"
          :label="$t('project.tasks.delete')"
          @click="remove"
        />
      </header>

      <q-card flat bordered>
        <q-card-section class="task-manage__fields">
          <q-input
            v-model="title"
            outlined
            counter
            maxlength="200"
            :label="$t('project.tasks.title')"
            :placeholder="$t('project.tasks.titlePlaceholder')"
          />
          <MarkdownEditor
            v-model="description"
            :label="$t('project.tasks.introduction')"
            :placeholder="$t('project.tasks.descriptionPlaceholder')"
            :max-length="100000"
          />
        </q-card-section>
      </q-card>

      <section class="task-manage__files">
        <div>
          <div class="prts-label">{{ $t('project.tasks.files') }}</div>
          <h3>{{ $t('project.tasks.chooseFiles') }}</h3>
          <p>{{ $t('project.tasks.folderSelectionHint') }}</p>
        </div>
        <q-badge outline color="primary">
          {{ $t('project.tasks.selectedFiles', { count: selectedFileIds.length }) }}
        </q-badge>
      </section>
      <ProjectFileBrowser
        :project-id="projectId"
        :folders="folders"
        :files="files"
        selectable
        :selected-file-ids="selectedFileIds"
        @selection-change="selectedFileIds = $event"
      />

      <footer class="task-manage__actions">
        <q-btn
          flat
          no-caps
          :label="$t('project.cancel')"
          :to="isEdit
            ? { name: 'project-task-detail', params: { id: projectId, taskId } }
            : { name: 'project-tasks', params: { id: projectId } }"
        />
        <q-btn
          unelevated
          no-caps
          color="primary"
          text-color="dark"
          icon="mdi-content-save-outline"
          :disable="!valid"
          :loading="saving"
          :label="$t('project.save')"
          @click="save"
        />
      </footer>
    </template>
    <div v-else-if="!loading" class="prts-empty">{{ $t('project.tasks.noManagePermission') }}</div>
  </section>
</template>

<style scoped>
.task-manage,
.task-manage__header > div,
.task-manage__fields {
  display: grid;
}

.task-manage {
  position: relative;
  gap: 16px;
}

.task-manage__header,
.task-manage__files,
.task-manage__actions {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 18px;
}

.task-manage__header > div,
.task-manage__fields {
  gap: 12px;
}

.task-manage__header h2,
.task-manage__header p,
.task-manage__files h3,
.task-manage__files p {
  margin: 0;
}

.task-manage__header h2 {
  color: var(--prts-text-strong);
  font: 500 22px var(--font-display);
}

.task-manage__header p,
.task-manage__files p {
  color: var(--prts-text-dim);
}

.task-manage__files {
  align-items: end;
  margin-top: 8px;
}

.task-manage__files h3 {
  color: var(--prts-text-strong);
  font: 600 16px var(--font-display);
}

.task-manage__actions {
  justify-content: flex-end;
  padding-top: 6px;
}
</style>
