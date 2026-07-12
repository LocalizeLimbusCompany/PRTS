<script setup lang="ts">
import { ref, watch } from 'vue'
import { useQuasar } from 'quasar'
import { useI18n } from 'vue-i18n'
import { useRouter } from 'vue-router'

import { apiErrorMessage, projectsApi } from '@/api'
import MarkdownEditor from '@/components/MarkdownEditor.vue'
import LegacyProjectControls from '@/components/project/LegacyProjectControls.vue'
import { hasProjectCapability } from '@/lib/capabilities'
import { useProjectWorkspace } from '@/lib/projectWorkspace'

const { detail, projectId, reload } = useProjectWorkspace()
const $q = useQuasar()
const { t } = useI18n()
const router = useRouter()
const saving = ref(false)
const form = ref({ name: '', description: '', visibility: 'public' })

watch(
  () => detail.value?.project,
  (project) => {
    if (!project) return
    if (!hasProjectCapability(detail.value?.capabilities, 'manage_project')) {
      void router.replace({ name: 'project-info', params: { id: projectId.value } })
      return
    }
    form.value = {
      name: project.name,
      description: project.description,
      visibility: project.visibility,
    }
  },
  { immediate: true },
)

/** Save only mature metadata; language changes remain gated until Task 2.2. */
async function save() {
  if (!form.value.name.trim()) return
  saving.value = true
  try {
    await projectsApi.update(projectId.value, {
      name: form.value.name.trim(),
      description: form.value.description,
      visibility: form.value.visibility,
    })
    await reload()
    $q.notify({ type: 'positive', message: t('project.manage.saved') })
  } catch (error) {
    $q.notify({ type: 'negative', message: apiErrorMessage(error) })
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <section
    v-if="hasProjectCapability(detail?.capabilities, 'manage_project')"
    class="manage-view"
  >
    <div>
      <div class="prts-label">{{ $t('project.sections.manage') }}</div>
      <h2>{{ $t('project.manage.heading') }}</h2>
    </div>

    <q-card flat bordered>
      <q-card-section class="manage-view__form">
        <div class="prts-label">{{ $t('project.manage.information') }}</div>
        <q-input v-model="form.name" outlined :label="$t('project.manage.name')" />
        <q-input
          :model-value="detail?.project.slug"
          outlined
          readonly
          :label="$t('project.slug')"
          hint="Slug"
        />
        <q-select
          v-model="form.visibility"
          outlined
          emit-value
          map-options
          :options="[
            { label: $t('project.public'), value: 'public' },
            { label: $t('project.private'), value: 'private' },
          ]"
          :label="$t('project.visibility')"
        />
        <MarkdownEditor
          v-model="form.description"
          class="manage-view__description"
          :label="$t('project.description')"
          :placeholder="$t('project.manage.descriptionPlaceholder')"
        />
        <div class="manage-view__actions">
          <span class="prts-dim">{{ $t('project.manage.languageNotice') }}</span>
          <q-btn
            unelevated
            no-caps
            color="primary"
            text-color="dark"
            icon="mdi-content-save-outline"
            :label="$t('project.save')"
            :loading="saving"
            @click="save"
          />
        </div>
      </q-card-section>
    </q-card>

    <LegacyProjectControls />
  </section>
</template>

<style scoped>
.manage-view,
.manage-view__form {
  display: grid;
  gap: 18px;
}

.manage-view h2 {
  margin: 4px 0 0;
  color: var(--prts-text-strong);
  font: 500 22px var(--font-display);
}

.manage-view__form {
  grid-template-columns: 1fr 1fr;
}

.manage-view__form > .prts-label,
.manage-view__description,
.manage-view__actions {
  grid-column: 1 / -1;
}

.manage-view__actions {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 18px;
}

@media (max-width: 720px) {
  .manage-view__form {
    grid-template-columns: 1fr;
  }
}
</style>
