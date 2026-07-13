<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useQuasar } from 'quasar'
import { useI18n } from 'vue-i18n'

import { apiErrorMessage, projectsApi, type FolderDto } from '@/api'

export interface FileMoveTarget {
  kind: 'file' | 'folder'
  id: number
  name: string
  parentId: number | null
}

const props = defineProps<{
  modelValue: boolean
  projectId: number
  target: FileMoveTarget | null
  folders: FolderDto[]
}>()

const emit = defineEmits<{
  'update:modelValue': [value: boolean]
  saved: []
}>()

const $q = useQuasar()
const { t } = useI18n()
const name = ref('')
const destinationId = ref<number | null>(null)
const saving = ref(false)

const open = computed({
  get: () => props.modelValue,
  set: (value) => emit('update:modelValue', value),
})

const folderById = computed(() => new Map(props.folders.map((folder) => [folder.id, folder])))
const destinationOptions = computed(() => [
  { label: t('project.files.root'), value: null },
  ...props.folders
    .filter((folder) => folder.id !== props.target?.id && !isTargetDescendant(folder))
    .map((folder) => ({ label: folder.path, value: folder.id })),
])

/** Exclude the moving folder's descendants from the picker; the server remains authoritative. */
function isTargetDescendant(folder: FolderDto): boolean {
  if (props.target?.kind !== 'folder') return false
  let current: FolderDto | undefined = folder
  while (current) {
    if (current.parent_id === props.target.id) return true
    current = current.parent_id === null ? undefined : folderById.value.get(current.parent_id)
  }
  return false
}

watch(
  () => [props.modelValue, props.target] as const,
  ([visible, target]) => {
    if (!visible || !target) return
    name.value = target.name
    destinationId.value = target.parentId
  },
  { immediate: true },
)

/** Submit the complete expected name and parent so move and rename share one atomic endpoint. */
async function save() {
  const target = props.target
  const normalizedName = name.value.trim()
  if (!target || !normalizedName) return
  saving.value = true
  try {
    if (target.kind === 'file') {
      await projectsApi.moveFile(props.projectId, target.id, {
        folder_id: destinationId.value,
        name: normalizedName,
      })
    } else {
      await projectsApi.moveFolder(props.projectId, target.id, {
        parent_id: destinationId.value,
        name: normalizedName,
      })
    }
    open.value = false
    emit('saved')
    $q.notify({ type: 'positive', message: t('project.files.moveSaved') })
  } catch (error) {
    $q.notify({ type: 'negative', message: apiErrorMessage(error) })
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <q-dialog v-model="open">
    <q-card class="file-move-dialog">
      <q-card-section>
        <div class="prts-label">{{ $t('project.files.organize') }}</div>
        <div class="prts-h2">{{ $t('project.files.moveHeading') }}</div>
      </q-card-section>
      <q-card-section class="file-move-dialog__form">
        <q-input
          v-model="name"
          outlined
          autofocus
          :label="$t('project.files.newName')"
          @keyup.enter="save"
        />
        <q-select
          v-model="destinationId"
          outlined
          emit-value
          map-options
          :options="destinationOptions"
          :label="$t('project.files.destination')"
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
          :loading="saving"
          :disable="!name.trim()"
          @click="save"
        />
      </q-card-actions>
    </q-card>
  </q-dialog>
</template>

<style scoped>
.file-move-dialog {
  width: min(520px, 94vw);
}

.file-move-dialog__form {
  display: grid;
  gap: 14px;
}
</style>
