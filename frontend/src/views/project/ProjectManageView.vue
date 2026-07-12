<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useQuasar } from 'quasar'
import { useI18n } from 'vue-i18n'
import { useRouter } from 'vue-router'

import { apiErrorMessage, projectsApi } from '@/api'
import MarkdownEditor from '@/components/MarkdownEditor.vue'
import AvatarCropDialog from '@/components/project/AvatarCropDialog.vue'
import LegacyProjectControls from '@/components/project/LegacyProjectControls.vue'
import LanguageResolutionDialog from '@/components/project/LanguageResolutionDialog.vue'
import ProjectAvatar from '@/components/project/ProjectAvatar.vue'
import { useJobProgress } from '@/composables/useJobProgress'
import { hasProjectCapability } from '@/lib/capabilities'
import { useProjectWorkspace } from '@/lib/projectWorkspace'

const { detail, projectId, reload } = useProjectWorkspace()
const $q = useQuasar()
const { t } = useI18n()
const router = useRouter()
const saving = ref(false)
const form = ref({ name: '', description: '', visibility: 'public' })
const changingPrimary = ref(false)
const showResolution = ref(false)
const languageForm = ref({ source_langs: [] as string[], primary_source_lang: '' })
const avatarFile = ref<File | null>(null)
const showAvatarCrop = ref(false)
const changingAvatar = ref(false)

const lexicalJobId = computed(() => detail.value?.project.lexical_job_id)
const embeddingJobId = computed(() => detail.value?.project.embedding_job_id)
const lexicalProgress = useJobProgress(lexicalJobId)
const embeddingProgress = useJobProgress(embeddingJobId)
const cooldownActive = computed(() => {
  const until = detail.value?.project.primary_source_cooldown_until
  return until ? new Date(until).getTime() > Date.now() : false
})
const cooldownLabel = computed(() => {
  const until = detail.value?.project.primary_source_cooldown_until
  return until ? new Date(until).toLocaleString() : ''
})

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
    languageForm.value = {
      source_langs: [...project.source_langs],
      primary_source_lang: project.primary_source_lang ?? project.source_langs[0] ?? '',
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

/** Submit primary-source changes only through the dedicated rebuild endpoint. */
async function changePrimarySource() {
  if (!languageForm.value.primary_source_lang) return
  changingPrimary.value = true
  try {
    await projectsApi.changePrimarySource(projectId.value, languageForm.value)
    await reload()
    $q.notify({ type: 'positive', message: t('project.language.changeQueued') })
  } catch (error) {
    $q.notify({ type: 'negative', message: apiErrorMessage(error) })
  } finally {
    changingPrimary.value = false
  }
}

async function retryStage(stage: 'lexical' | 'embedding') {
  try {
    if (stage === 'lexical') await lexicalProgress.retry()
    else await embeddingProgress.retry()
    await reload()
  } catch (error) {
    $q.notify({ type: 'negative', message: apiErrorMessage(error) })
  }
}

async function resolvedLanguages() {
  await reload()
}

function selectAvatar(file: File | null) {
  avatarFile.value = file
  showAvatarCrop.value = Boolean(file)
}

async function uploadAvatar(blob: Blob) {
  changingAvatar.value = true
  try {
    await projectsApi.uploadAvatar(projectId.value, blob)
    avatarFile.value = null
    await reload()
    $q.notify({ type: 'positive', message: t('project.avatar.updated') })
  } catch (error) {
    $q.notify({ type: 'negative', message: apiErrorMessage(error) })
  } finally {
    changingAvatar.value = false
  }
}

async function deleteAvatar() {
  changingAvatar.value = true
  try {
    await projectsApi.deleteAvatar(projectId.value)
    await reload()
    $q.notify({ type: 'positive', message: t('project.avatar.deleted') })
  } catch (error) {
    $q.notify({ type: 'negative', message: apiErrorMessage(error) })
  } finally {
    changingAvatar.value = false
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
        <div class="manage-view__avatar">
          <ProjectAvatar
            v-if="detail"
            :project-id="detail.project.id"
            :name="detail.project.name"
            :avatar-url="detail.project.avatar_url"
            :avatar-updated-at="detail.project.avatar_updated_at"
            size="88px"
          />
          <div class="manage-view__avatar-copy">
            <strong>{{ $t('project.avatar.heading') }}</strong>
            <span class="prts-dim">{{ $t('project.avatar.description') }}</span>
            <div class="row q-gutter-sm">
              <q-file
                :model-value="avatarFile"
                class="manage-view__avatar-file"
                dense
                outlined
                accept="image/*"
                :label="$t('project.avatar.choose')"
                :disable="changingAvatar"
                @update:model-value="selectAvatar"
              >
                <template #prepend><q-icon name="mdi-image-plus-outline" /></template>
              </q-file>
              <q-btn
                v-if="detail?.project.avatar_url"
                flat
                no-caps
                color="negative"
                icon="mdi-delete-outline"
                :label="$t('project.avatar.remove')"
                :loading="changingAvatar"
                @click="deleteAvatar"
              />
            </div>
          </div>
        </div>
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

    <q-card flat bordered>
      <q-card-section class="manage-view__language">
        <div class="manage-view__language-head">
          <div>
            <div class="prts-label">{{ $t('project.language.heading') }}</div>
            <div class="prts-dim q-mt-xs">{{ $t('project.language.description') }}</div>
          </div>
          <q-btn
            v-if="
              hasProjectCapability(detail?.capabilities, 'resolve_languages') &&
              detail?.project.language_repair_state !== 'ready'
            "
            outline
            no-caps
            color="warning"
            icon="mdi-source-branch-sync"
            :label="$t('project.language.resolveAction')"
            @click="showResolution = true"
          />
        </div>

        <div class="manage-view__stages">
          <div class="manage-view__stage">
            <div class="row items-center">
              <q-icon name="mdi-text-search" color="primary" size="20px" />
              <strong>{{ $t('project.language.lexical') }}</strong>
              <q-space />
              <q-badge outline :label="detail?.project.lexical_state" />
            </div>
            <q-linear-progress
              :value="lexicalProgress.progress.value ?? 0"
              :indeterminate="
                lexicalProgress.active.value && lexicalProgress.progress.value === null
              "
              size="5px"
              color="primary"
            />
            <q-btn
              v-if="lexicalProgress.job.value?.manual_retry_allowed"
              flat
              dense
              no-caps
              icon="mdi-refresh"
              :label="$t('project.language.retry')"
              :loading="lexicalProgress.loading.value"
              @click="retryStage('lexical')"
            />
          </div>
          <div class="manage-view__stage">
            <div class="row items-center">
              <q-icon name="mdi-vector-polyline" color="secondary" size="20px" />
              <strong>{{ $t('project.language.embedding') }}</strong>
              <q-space />
              <q-badge outline :label="detail?.project.embedding_state" />
            </div>
            <q-linear-progress
              :value="embeddingProgress.progress.value ?? 0"
              :indeterminate="
                embeddingProgress.active.value && embeddingProgress.progress.value === null
              "
              size="5px"
              color="secondary"
            />
            <span v-if="detail?.project.embedding_degraded_reason" class="prts-dim">
              {{ $t(`project.language.reasons.${detail.project.embedding_degraded_reason}`) }}
            </span>
            <q-btn
              v-if="embeddingProgress.job.value?.manual_retry_allowed"
              flat
              dense
              no-caps
              icon="mdi-refresh"
              :label="$t('project.language.retry')"
              :loading="embeddingProgress.loading.value"
              @click="retryStage('embedding')"
            />
          </div>
        </div>

        <div
          v-if="hasProjectCapability(detail?.capabilities, 'change_primary_source')"
          class="manage-view__language-form"
        >
          <q-select
            v-model="languageForm.source_langs"
            outlined
            multiple
            use-chips
            use-input
            new-value-mode="add-unique"
            :options="languageForm.source_langs"
            :label="$t('project.sourceLanguages')"
          />
          <q-select
            v-model="languageForm.primary_source_lang"
            outlined
            emit-value
            :options="languageForm.source_langs"
            :label="$t('project.primarySource')"
          />
          <div class="manage-view__language-action">
            <span v-if="cooldownActive" class="prts-dim">
              {{ $t('project.language.cooldownUntil', { time: cooldownLabel }) }}
            </span>
            <span v-else class="prts-dim">{{ $t('project.language.cooldownHint') }}</span>
            <q-btn
              unelevated
              no-caps
              color="primary"
              text-color="dark"
              icon="mdi-source-branch-sync"
              :label="$t('project.language.changeAction')"
              :loading="changingPrimary"
              :disable="
                cooldownActive ||
                languageForm.primary_source_lang === detail?.project.primary_source_lang
              "
              @click="changePrimarySource"
            />
          </div>
        </div>
      </q-card-section>
    </q-card>

    <LegacyProjectControls />
    <LanguageResolutionDialog
      v-model="showResolution"
      :project-id="projectId"
      @resolved="resolvedLanguages"
    />
    <AvatarCropDialog
      v-model="showAvatarCrop"
      :file="avatarFile"
      @cropped="uploadAvatar"
    />
  </section>
</template>

<style scoped>
.manage-view,
.manage-view__form {
  display: grid;
  gap: 18px;
}

.manage-view__language,
.manage-view__stage,
.manage-view__language-form {
  display: grid;
  gap: 14px;
}

.manage-view__language-head,
.manage-view__language-action {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 18px;
}

.manage-view__stages,
.manage-view__language-form {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 12px;
}

.manage-view__stage {
  padding: 14px;
  border: 1px solid var(--prts-border);
  background: var(--prts-panel-2);
}

.manage-view__language-form {
  padding-top: 14px;
  border-top: 1px solid var(--prts-border);
}

.manage-view__language-action {
  grid-column: 1 / -1;
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
.manage-view__avatar,
.manage-view__description,
.manage-view__actions {
  grid-column: 1 / -1;
}

.manage-view__avatar {
  display: flex;
  align-items: center;
  gap: 16px;
  padding: 14px;
  border: 1px solid var(--prts-border-soft);
  background: var(--prts-panel-2);
}

.manage-view__avatar-copy {
  display: grid;
  min-width: 0;
  flex: 1;
  gap: 7px;
}

.manage-view__avatar-file {
  width: min(360px, 100%);
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

  .manage-view__stages,
  .manage-view__language-form {
    grid-template-columns: 1fr;
  }

  .manage-view__avatar {
    align-items: flex-start;
    flex-direction: column;
  }
}
</style>
