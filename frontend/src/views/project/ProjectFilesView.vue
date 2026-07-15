<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useQuasar } from 'quasar'
import { useI18n } from 'vue-i18n'

import { apiErrorMessage, projectsApi, type FileDto, type FolderDto } from '@/api'
import FileHistoryDialog from '@/components/project/FileHistoryDialog.vue'
import FileMoveDialog, { type FileMoveTarget } from '@/components/project/FileMoveDialog.vue'
import ProjectFileBrowser from '@/components/project/ProjectFileBrowser.vue'
import UploadBatchDialog from '@/components/project/UploadBatchDialog.vue'
import { hasProjectCapability } from '@/lib/capabilities'
import type { FileHistoryTarget } from '@/lib/fileHistory'
import type { ProjectBrowserItem } from '@/lib/projectFiles'
import { useProjectWorkspace } from '@/lib/projectWorkspace'

const { detail, projectId, reload } = useProjectWorkspace()
const $q = useQuasar()
const { t } = useI18n()
const folders = ref<FolderDto[]>([])
const files = ref<FileDto[]>([])
const loading = ref(true)
const showUpload = ref(false)
const showMove = ref(false)
const moveTarget = ref<FileMoveTarget | null>(null)
const showHistory = ref(false)
const historyTarget = ref<FileHistoryTarget | null>(null)
const showCreateFolder = ref(false)
const createFolderParentId = ref<number | null>(null)
const folderName = ref('')
const creatingFolder = ref(false)

const canManage = computed(() => hasProjectCapability(detail.value?.capabilities, 'manage_project'))
const canUpload = computed(() => hasProjectCapability(detail.value?.capabilities, 'upload_files'))
const canViewHistory = computed(() =>
  hasProjectCapability(detail.value?.capabilities, 'view_file_history'),
)
const canRollback = computed(() =>
  hasProjectCapability(detail.value?.capabilities, 'rollback_file_history'),
)

/** Fetch the read-only tree; all filtering and sorting stays in the browser. */
async function load() {
  loading.value = true
  try {
    const tree = await projectsApi.tree(projectId.value)
    folders.value = tree.folders
    files.value = tree.files
  } catch (error) {
    $q.notify({ type: 'negative', message: apiErrorMessage(error) })
  } finally {
    loading.value = false
  }
}

/** Refresh both the active tree and materialized workspace statistics after a mutation. */
async function refresh() {
  await Promise.all([load(), reload()])
}

function createFolder(parentId: number | null) {
  createFolderParentId.value = parentId
  folderName.value = ''
  showCreateFolder.value = true
}

async function saveFolder() {
  const name = folderName.value.trim()
  if (!name) return
  creatingFolder.value = true
  try {
    await projectsApi.createFolder(projectId.value, {
      parent_id: createFolderParentId.value,
      name,
    })
    showCreateFolder.value = false
    await refresh()
    $q.notify({ type: 'positive', message: t('project.files.folderCreated') })
  } catch (error) {
    $q.notify({ type: 'negative', message: apiErrorMessage(error) })
  } finally {
    creatingFolder.value = false
  }
}

function move(item: ProjectBrowserItem) {
  moveTarget.value = {
    kind: item.kind,
    id: item.id,
    name: item.name,
    parentId: item.folderId,
  }
  showMove.value = true
}

function history(item?: ProjectBrowserItem) {
  historyTarget.value = item ? { kind: item.kind, id: item.id } : null
  showHistory.value = true
}

/** Soft deletion remains recoverable through its history operation for thirty days. */
function remove(item: ProjectBrowserItem) {
  $q.dialog({
    title: t('project.files.delete'),
    message: t('project.files.deleteConfirm', { path: item.path }),
    cancel: true,
    ok: { label: t('project.files.delete'), color: 'negative', noCaps: true },
  }).onOk(async () => {
    try {
      if (item.kind === 'file') await projectsApi.deleteFile(projectId.value, item.id)
      else await projectsApi.deleteFolder(projectId.value, item.id)
      await refresh()
      $q.notify({ type: 'positive', message: t('project.files.deleted') })
    } catch (error) {
      $q.notify({ type: 'negative', message: apiErrorMessage(error) })
    }
  })
}

onMounted(load)
</script>

<template>
  <section>
    <div class="project-section-heading">
      <div>
        <div class="prts-label">{{ $t('project.sections.files') }}</div>
        <h2>{{ $t('project.files.heading') }}</h2>
      </div>
      <div class="row q-gutter-sm items-center">
        <span class="prts-mono prts-dim">{{ files.length }} {{ $t('project.files.count') }}</span>
        <q-btn
          v-if="canViewHistory"
          outline
          no-caps
          icon="mdi-history"
          :label="$t('project.files.projectHistory')"
          @click="history()"
        />
        <q-btn
          v-if="canUpload"
          unelevated
          no-caps
          color="primary"
          text-color="dark"
          icon="mdi-upload"
          :label="$t('project.upload.action')"
          @click="showUpload = true"
        />
      </div>
    </div>
    <q-skeleton v-if="loading" height="320px" square />
    <ProjectFileBrowser
      v-else
      :project-id="projectId"
      :folders="folders"
      :files="files"
      :can-manage="canManage"
      :can-view-history="canViewHistory"
      @create-folder="createFolder"
      @move="move"
      @history="history"
      @delete="remove"
    />
    <UploadBatchDialog
      v-if="canUpload"
      v-model="showUpload"
      :project-id="projectId"
      :folders="folders"
      @completed="refresh"
    />
    <FileMoveDialog
      v-if="canManage"
      v-model="showMove"
      :project-id="projectId"
      :target="moveTarget"
      :folders="folders"
      @saved="refresh"
    />
    <FileHistoryDialog
      v-if="canViewHistory"
      v-model="showHistory"
      :project-id="projectId"
      :target="historyTarget"
      :can-rollback="canRollback"
      @changed="refresh"
    />
    <q-dialog v-model="showCreateFolder">
      <q-card style="width: min(460px, 94vw)">
        <q-card-section>
          <div class="prts-label">{{ $t('project.files.organize') }}</div>
          <div class="prts-h2">{{ $t('project.files.createFolder') }}</div>
        </q-card-section>
        <q-card-section>
          <q-input
            v-model="folderName"
            outlined
            autofocus
            :label="$t('project.files.folderName')"
            @keyup.enter="saveFolder"
          />
        </q-card-section>
        <q-card-actions align="right">
          <q-btn v-close-popup flat no-caps :label="$t('project.cancel')" />
          <q-btn
            unelevated
            no-caps
            color="primary"
            text-color="dark"
            :label="$t('project.save')"
            :disable="!folderName.trim()"
            :loading="creatingFolder"
            @click="saveFolder"
          />
        </q-card-actions>
      </q-card>
    </q-dialog>
  </section>
</template>

<style scoped>
.project-section-heading {
  display: flex;
  align-items: end;
  justify-content: space-between;
  gap: 20px;
  margin-bottom: 16px;
}

.project-section-heading h2 {
  margin: 4px 0 0;
  color: var(--prts-text-strong);
  font: 500 22px var(--font-display);
}
</style>
