<script setup lang="ts">
import { ref } from 'vue'
import { useQuasar } from 'quasar'

import { apiErrorMessage, projectsApi } from '@/api'
import { hasProjectCapability } from '@/lib/capabilities'
import { useProjectWorkspace } from '@/lib/projectWorkspace'

const { detail, projectId } = useProjectWorkspace()
const $q = useQuasar()
const exporting = ref(false)

/** Download the current project export without navigating away. */
async function download() {
  exporting.value = true
  try {
    const blob = await projectsApi.exportProject(projectId.value)
    const url = URL.createObjectURL(blob)
    const anchor = document.createElement('a')
    anchor.href = url
    anchor.download = `${detail.value?.project.slug ?? 'project'}.zip`
    anchor.click()
    URL.revokeObjectURL(url)
  } catch (error) {
    $q.notify({ type: 'negative', message: apiErrorMessage(error) })
  } finally {
    exporting.value = false
  }
}
</script>

<template>
  <section class="download-view">
    <div class="prts-label">{{ $t('project.sections.download') }}</div>
    <h2>{{ $t('project.download.heading') }}</h2>
    <q-card flat bordered>
      <q-card-section class="download-view__card">
        <q-icon name="mdi-archive-arrow-down-outline" size="38px" color="primary" />
        <div>
          <div class="prts-h2">{{ $t('project.download.archive') }}</div>
          <p class="prts-dim">{{ $t('project.download.description') }}</p>
        </div>
        <q-space />
        <q-btn
          v-if="hasProjectCapability(detail?.capabilities, 'download')"
          unelevated
          no-caps
          color="primary"
          text-color="dark"
          icon="mdi-download"
          :label="$t('project.download.action')"
          :loading="exporting"
          @click="download"
        />
        <span v-else class="prts-dim">{{ $t('project.download.noPermission') }}</span>
      </q-card-section>
    </q-card>
  </section>
</template>

<style scoped>
.download-view {
  display: grid;
  gap: 14px;
}

.download-view h2 {
  margin: -8px 0 4px;
  color: var(--prts-text-strong);
  font: 500 22px var(--font-display);
}

.download-view__card {
  display: flex;
  align-items: center;
  gap: 16px;
  min-height: 128px;
}

.download-view__card p {
  margin: 5px 0 0;
}
</style>
