<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useQuasar } from 'quasar'
import { useI18n } from 'vue-i18n'
import { useRouter } from 'vue-router'

import {
  aiApi,
  apiErrorMessage,
  projectsApi,
  type AiSettingsDto,
  type MemberDto,
} from '@/api'
import MarkdownEditor from '@/components/MarkdownEditor.vue'
import AvatarCropDialog from '@/components/project/AvatarCropDialog.vue'
import LanguageResolutionDialog from '@/components/project/LanguageResolutionDialog.vue'
import ProjectDeleteDialog from '@/components/project/ProjectDeleteDialog.vue'
import ProjectAvatar from '@/components/project/ProjectAvatar.vue'
import { useJobProgress } from '@/composables/useJobProgress'
import { hasProjectCapability } from '@/lib/capabilities'
import { roleLabel } from '@/lib/states'
import { useProjectWorkspace } from '@/lib/projectWorkspace'
import { useAuthStore } from '@/stores/auth'

const { detail, projectId, reload } = useProjectWorkspace()
const $q = useQuasar()
const { t } = useI18n()
const router = useRouter()
const auth = useAuthStore()
const saving = ref(false)
const form = ref({ name: '', description: '', visibility: 'public', comment_policy: 'private' })
const changingPrimary = ref(false)
const showResolution = ref(false)
const languageForm = ref({ source_langs: [] as string[], primary_source_lang: '' })
const avatarFile = ref<File | null>(null)
const showAvatarCrop = ref(false)
const changingAvatar = ref(false)
const members = ref<MemberDto[]>([])
const loadingMembers = ref(false)
const showAddMember = ref(false)
const savingMember = ref<number | 'new' | null>(null)
const newMember = ref({ username: '', role: '' })
const showDeleteDialog = ref(false)
const cancellingDeletion = ref(false)
const projectAiSettings = ref<AiSettingsDto | null>(null)
const projectAiForm = ref({ base_url: '', model: '', api_key: '', enabled: true })
const projectAiSaving = ref(false)
const countdownNow = ref(Date.now())
let countdownTimer: ReturnType<typeof setInterval> | undefined

const lexicalJobId = computed(() => detail.value?.project.lexical_job_id)
const embeddingJobId = computed(() => detail.value?.project.embedding_job_id)
const isProjectOwner = computed(() => detail.value?.project.owner_id === auth.user?.id)
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
const deletionCountdown = computed(() => {
  const deadline = detail.value?.project.deletion_scheduled_at
  if (!deadline) return ''
  const remaining = Math.max(0, new Date(deadline).getTime() - countdownNow.value)
  const hours = Math.floor(remaining / 3_600_000)
  const minutes = Math.floor((remaining % 3_600_000) / 60_000)
  return `${hours}h ${minutes}m`
})

watch(
  () => [detail.value?.project, auth.user?.id] as const,
  ([project, userId]) => {
    if (!project) return
    if (
      !project.deletion_scheduled_at &&
      !hasProjectCapability(detail.value?.capabilities, 'manage_project')
    ) {
      void router.replace({ name: 'project-info', params: { id: projectId.value } })
      return
    }
    form.value = {
      name: project.name,
      description: project.description,
      visibility: project.visibility,
      comment_policy: project.comment_policy,
    }
    languageForm.value = {
      source_langs: [...project.source_langs],
      primary_source_lang: project.primary_source_lang ?? project.source_langs[0] ?? '',
    }
    if (project.owner_id === userId) void loadProjectAi()
  },
  { immediate: true },
)

function applyProjectAiSettings(setting: AiSettingsDto) {
  projectAiSettings.value = setting
  projectAiForm.value = {
    base_url: setting.base_url ?? '',
    model: setting.model ?? '',
    api_key: '',
    enabled: setting.configured ? setting.enabled : true,
  }
}

async function loadProjectAi() {
  try {
    applyProjectAiSettings(await aiApi.getProjectSettings(projectId.value))
  } catch (error) {
    $q.notify({ type: 'negative', message: apiErrorMessage(error) })
  }
}

async function saveProjectAi() {
  if (!projectAiForm.value.base_url.trim() || !projectAiForm.value.model.trim()) return
  projectAiSaving.value = true
  try {
    const apiKey = projectAiForm.value.api_key.trim()
    const updated = await aiApi.putProjectSettings(projectId.value, {
      base_url: projectAiForm.value.base_url.trim(),
      model: projectAiForm.value.model.trim(),
      api_key: apiKey || undefined,
      enabled: projectAiForm.value.enabled,
    })
    applyProjectAiSettings(updated)
    $q.notify({ type: 'positive', message: t('project.ai.saved') })
  } catch (error) {
    $q.notify({ type: 'negative', message: apiErrorMessage(error, t('project.ai.saveFailed')) })
  } finally {
    projectAiSaving.value = false
  }
}

function deleteProjectAi() {
  $q.dialog({
    title: t('project.ai.delete'),
    message: t('project.ai.deleteConfirm'),
    cancel: true,
  }).onOk(async () => {
    projectAiSaving.value = true
    try {
      await aiApi.deleteProjectSettings(projectId.value)
      applyProjectAiSettings({
        configured: false,
        base_url: null,
        model: null,
        api_key_hint: null,
        enabled: false,
      })
      $q.notify({ type: 'positive', message: t('project.ai.deleted') })
    } catch (error) {
      $q.notify({ type: 'negative', message: apiErrorMessage(error) })
    } finally {
      projectAiSaving.value = false
    }
  })
}

/** Save only mature metadata; language changes remain gated until Task 2.2. */
async function save() {
  if (!form.value.name.trim()) return
  saving.value = true
  try {
    await projectsApi.update(projectId.value, {
      name: form.value.name.trim(),
      description: form.value.description,
      visibility: form.value.visibility,
      comment_policy: form.value.comment_policy,
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

/** Refresh server-authored per-target membership capabilities. */
async function loadMembers() {
  loadingMembers.value = true
  try {
    members.value = await projectsApi.members(projectId.value)
  } catch (error) {
    $q.notify({ type: 'negative', message: apiErrorMessage(error) })
  } finally {
    loadingMembers.value = false
  }
}

function openAddMember() {
  const firstRole = detail.value?.capabilities.member_assignable_roles[0]
  if (!firstRole) return
  newMember.value = { username: '', role: firstRole }
  showAddMember.value = true
}

async function addMember() {
  if (!newMember.value.username.trim() || !newMember.value.role) return
  savingMember.value = 'new'
  try {
    await projectsApi.addMember(projectId.value, {
      username: newMember.value.username.trim(),
      role: newMember.value.role,
    })
    showAddMember.value = false
    await loadMembers()
    $q.notify({ type: 'positive', message: t('project.members.saved') })
  } catch (error) {
    $q.notify({ type: 'negative', message: apiErrorMessage(error) })
  } finally {
    savingMember.value = null
  }
}

async function changeMemberRole(member: MemberDto, role: string) {
  savingMember.value = member.user_id
  try {
    await projectsApi.addMember(projectId.value, { username: member.username, role })
    await loadMembers()
    $q.notify({ type: 'positive', message: t('project.members.saved') })
  } catch (error) {
    $q.notify({ type: 'negative', message: apiErrorMessage(error) })
  } finally {
    savingMember.value = null
  }
}

async function removeMember(member: MemberDto) {
  savingMember.value = member.user_id
  try {
    await projectsApi.removeMember(projectId.value, member.user_id)
    await loadMembers()
    $q.notify({ type: 'positive', message: t('project.members.removed') })
  } catch (error) {
    $q.notify({ type: 'negative', message: apiErrorMessage(error) })
  } finally {
    savingMember.value = null
  }
}

onMounted(() => {
  void loadMembers()
  countdownTimer = setInterval(() => {
    countdownNow.value = Date.now()
  }, 60_000)
})

onBeforeUnmount(() => {
  if (countdownTimer) clearInterval(countdownTimer)
})

async function cancelDeletion() {
  cancellingDeletion.value = true
  try {
    await projectsApi.cancelDeletion(projectId.value)
    await reload()
    $q.notify({ type: 'positive', message: t('project.deletion.cancelled') })
  } catch (error) {
    $q.notify({ type: 'negative', message: apiErrorMessage(error) })
  } finally {
    cancellingDeletion.value = false
  }
}
</script>

<template>
  <section
    v-if="
      detail?.project.deletion_scheduled_at ||
      hasProjectCapability(detail?.capabilities, 'manage_project')
    "
    class="manage-view"
  >
    <div>
      <div class="prts-label">{{ $t('project.sections.manage') }}</div>
      <h2>{{ $t('project.manage.heading') }}</h2>
    </div>

    <q-card v-if="detail?.project.deletion_scheduled_at" flat bordered class="manage-view__pending">
      <q-card-section class="column q-gutter-md">
        <div class="prts-label text-negative">{{ $t('project.deletion.pending') }}</div>
        <div class="prts-h2">{{ deletionCountdown }}</div>
        <p class="prts-dim">{{ $t('project.deletion.pendingReadonly') }}</p>
        <q-btn
          outline
          no-caps
          color="negative"
          :loading="cancellingDeletion"
          :label="$t('project.deletion.cancelDeletion')"
          @click="cancelDeletion"
        />
      </q-card-section>
    </q-card>

    <template v-else>
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
          <q-select
            v-model="form.comment_policy"
            outlined
            emit-value
            map-options
            :options="[
              { label: $t('project.comments.private'), value: 'private' },
              { label: $t('project.comments.internal'), value: 'internal' },
              { label: $t('project.comments.public'), value: 'public' },
            ]"
            :label="$t('project.comments.policy')"
            :hint="$t(`project.comments.${form.comment_policy}Hint`)"
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

      <q-card v-if="isProjectOwner" flat bordered>
        <q-card-section class="manage-view__ai">
          <div>
            <div class="prts-label">{{ $t('project.ai.heading') }}</div>
            <div class="prts-dim q-mt-xs">{{ $t('project.ai.description') }}</div>
          </div>
          <q-banner v-if="projectAiSettings?.configured" dense rounded class="bg-grey-9">
            {{
              $t('project.ai.keyConfigured', { hint: projectAiSettings.api_key_hint })
            }}
          </q-banner>
          <q-input
            v-model="projectAiForm.base_url"
            outlined
            type="url"
            autocomplete="url"
            :label="$t('profile.ai.baseUrl')"
            :hint="$t('profile.ai.baseUrlHint')"
            :disable="projectAiSaving"
          />
          <q-input
            v-model="projectAiForm.model"
            outlined
            :label="$t('profile.ai.model')"
            :disable="projectAiSaving"
          />
          <q-input
            v-model="projectAiForm.api_key"
            outlined
            type="password"
            autocomplete="new-password"
            :label="$t('profile.ai.apiKey')"
            :hint="
              projectAiSettings?.configured
                ? $t('profile.ai.apiKeyRetainHint')
                : $t('profile.ai.apiKeyRequiredHint')
            "
            :disable="projectAiSaving"
          />
          <q-toggle
            v-model="projectAiForm.enabled"
            :label="$t('profile.ai.enabled')"
            :disable="projectAiSaving"
          />
          <div class="row q-gutter-sm">
            <q-btn
              unelevated
              no-caps
              color="primary"
              text-color="dark"
              :label="$t('project.ai.save')"
              :loading="projectAiSaving"
              :disable="
                !projectAiForm.base_url.trim() ||
                !projectAiForm.model.trim() ||
                (!projectAiSettings?.configured && !projectAiForm.api_key.trim())
              "
              @click="saveProjectAi"
            />
            <q-btn
              v-if="projectAiSettings?.configured"
              flat
              no-caps
              color="negative"
              icon="mdi-delete-outline"
              :label="$t('project.ai.delete')"
              :disable="projectAiSaving"
              @click="deleteProjectAi"
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

      <q-card flat bordered>
        <q-card-section class="row items-center">
          <div>
            <div class="prts-label">{{ $t('project.members.heading') }}</div>
            <div class="prts-dim q-mt-xs">
              {{ $t('project.members.count', { count: members.length }) }}
            </div>
          </div>
          <q-space />
          <q-btn
            v-if="detail?.capabilities.member_assignable_roles.length"
            flat
            no-caps
            icon="mdi-account-plus-outline"
            :label="$t('project.members.add')"
            @click="openAddMember"
          />
        </q-card-section>
        <q-separator />
        <q-list separator>
          <q-item v-for="member in members" :key="member.user_id">
            <q-item-section avatar>
              <q-avatar square size="34px" color="primary" text-color="dark">
                <img v-if="member.avatar_url" :src="member.avatar_url" alt="" />
                <span v-else>{{ member.username.slice(0, 2).toUpperCase() }}</span>
              </q-avatar>
            </q-item-section>
            <q-item-section>
              <q-item-label>{{ member.username }}</q-item-label>
              <q-item-label caption>{{ roleLabel(member.role, t) }}</q-item-label>
            </q-item-section>
            <q-item-section side>
              <div class="row items-center no-wrap q-gutter-sm">
                <q-select
                  v-if="member.capabilities.can_change_role"
                  :model-value="member.role"
                  dense
                  outlined
                  emit-value
                  map-options
                  :options="
                    member.capabilities.assignable_roles.map((role) => ({
                      value: role,
                      label: roleLabel(role, t),
                    }))
                  "
                  :loading="savingMember === member.user_id"
                  :label="$t('project.members.role')"
                  @update:model-value="changeMemberRole(member, $event)"
                />
                <q-btn
                  v-if="member.capabilities.can_remove"
                  flat
                  round
                  dense
                  color="negative"
                  icon="mdi-account-remove-outline"
                  :loading="savingMember === member.user_id"
                  :aria-label="$t('project.members.remove')"
                  @click="removeMember(member)"
                />
              </div>
            </q-item-section>
          </q-item>
          <q-item v-if="!loadingMembers && !members.length">
            <q-item-section class="prts-dim">{{ $t('project.members.empty') }}</q-item-section>
          </q-item>
        </q-list>
      </q-card>

      <q-card
        v-if="hasProjectCapability(detail?.capabilities, 'delete_project')"
        flat
        bordered
        class="manage-view__danger"
      >
        <q-card-section class="row items-center">
          <div>
            <div class="prts-label text-negative">{{ $t('project.deletion.label') }}</div>
            <div class="prts-dim q-mt-xs">{{ $t('project.deletion.dangerHint') }}</div>
          </div>
          <q-space />
          <q-btn
            outline
            no-caps
            color="negative"
            icon="mdi-delete-clock-outline"
            :label="$t('project.deletion.open')"
            @click="showDeleteDialog = true"
          />
        </q-card-section>
      </q-card>
    </template>
    <LanguageResolutionDialog
      v-model="showResolution"
      :project-id="projectId"
      @resolved="resolvedLanguages"
    />
    <AvatarCropDialog v-model="showAvatarCrop" :file="avatarFile" @cropped="uploadAvatar" />
    <q-dialog v-model="showAddMember">
      <q-card style="width: 420px; max-width: 92vw">
        <q-card-section
          ><div class="prts-h2">{{ $t('project.members.add') }}</div></q-card-section
        >
        <q-card-section class="column q-gutter-md">
          <q-input
            v-model="newMember.username"
            dense
            outlined
            :label="$t('project.members.username')"
          />
          <q-select
            v-model="newMember.role"
            dense
            outlined
            emit-value
            map-options
            :options="
              detail?.capabilities.member_assignable_roles.map((role) => ({
                value: role,
                label: roleLabel(role, t),
              })) ?? []
            "
            :label="$t('project.members.role')"
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
            :loading="savingMember === 'new'"
            @click="addMember"
          />
        </q-card-actions>
      </q-card>
    </q-dialog>
    <ProjectDeleteDialog
      v-if="detail"
      v-model="showDeleteDialog"
      :project-id="projectId"
      :slug="detail.project.slug"
      @scheduled="reload"
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
.manage-view__ai,
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

  .manage-view__actions,
  .manage-view__language-head,
  .manage-view__language-action {
    align-items: stretch;
    flex-direction: column;
  }
}
</style>
