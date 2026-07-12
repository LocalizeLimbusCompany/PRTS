<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { useQuasar } from 'quasar'

import { apiErrorMessage, projectsApi, type FileDto, type FolderDto } from '@/api'
import ProjectFileBrowser from '@/components/project/ProjectFileBrowser.vue'
import { useProjectWorkspace } from '@/lib/projectWorkspace'

const { projectId } = useProjectWorkspace()
const $q = useQuasar()
const folders = ref<FolderDto[]>([])
const files = ref<FileDto[]>([])
const loading = ref(true)

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

onMounted(load)
</script>

<template>
  <section>
    <div class="project-section-heading">
      <div>
        <div class="prts-label">{{ $t('project.sections.files') }}</div>
        <h2>{{ $t('project.files.heading') }}</h2>
      </div>
      <span class="prts-mono prts-dim">{{ files.length }} {{ $t('project.files.count') }}</span>
    </div>
    <q-skeleton v-if="loading" height="320px" square />
    <ProjectFileBrowser
      v-else
      :project-id="projectId"
      :folders="folders"
      :files="files"
    />
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
