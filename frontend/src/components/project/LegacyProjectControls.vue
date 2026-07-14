<script setup lang="ts">
import { useRouter } from 'vue-router'
import { useQuasar } from 'quasar'
import { useI18n } from 'vue-i18n'

import { apiErrorMessage, projectsApi } from '@/api'
import { hasProjectCapability } from '@/lib/capabilities'
import { useProjectWorkspace } from '@/lib/projectWorkspace'

const { detail, projectId } = useProjectWorkspace()
const router = useRouter()
const $q = useQuasar()
const { t } = useI18n()

function confirmDelete() {
  $q.dialog({
    title: t('project.legacy.deleteTitle'),
    message: t('project.legacy.deleteWarning', { name: detail.value?.project.name }),
    cancel: true,
    ok: { label: t('project.legacy.deleteAction'), color: 'negative', noCaps: true },
  }).onOk(async () => {
    try {
      await projectsApi.remove(projectId.value)
      await router.push({ name: 'projects' })
    } catch (error) {
      $q.notify({ type: 'negative', message: apiErrorMessage(error) })
    }
  })
}
</script>

<template>
  <section class="legacy-controls">
    <q-card
      v-if="hasProjectCapability(detail?.capabilities, 'delete_project')"
      flat
      bordered
      class="legacy-controls__danger"
    >
      <q-card-section class="legacy-controls__section">
        <div>
          <div class="prts-label text-negative">{{ $t('project.legacy.danger') }}</div>
          <div class="prts-dim q-mt-xs">{{ $t('project.legacy.deleteHint') }}</div>
        </div>
        <q-space />
        <q-btn
          outline
          no-caps
          color="negative"
          icon="mdi-delete-outline"
          :label="$t('project.legacy.deleteAction')"
          @click="confirmDelete"
        />
      </q-card-section>
    </q-card>
  </section>
</template>

<style scoped>
.legacy-controls,
.legacy-controls__section {
  display: grid;
  gap: 14px;
}

.legacy-controls__notice {
  border: 1px solid var(--prts-border);
  background: var(--prts-panel-2);
}

.legacy-controls__section {
  grid-template-columns: minmax(0, 1fr) auto auto;
  align-items: center;
}

.legacy-controls__danger {
  border-color: color-mix(in srgb, var(--prts-danger) 48%, var(--prts-border));
}

@media (max-width: 760px) {
  .legacy-controls__section {
    grid-template-columns: 1fr;
  }
}
</style>
