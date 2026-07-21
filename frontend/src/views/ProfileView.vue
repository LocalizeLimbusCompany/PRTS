<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useQuasar } from 'quasar'
import { useI18n } from 'vue-i18n'
import { useRoute, useRouter } from 'vue-router'

import {
  aiApi,
  apiErrorMessage,
  usersApi,
  type AiSettingsDto,
  type AiSettingsWriteRequest,
  type AiSourcePreference,
  type ApiKeyDto,
  type ApiKeyScope,
  type EntryDiffMode,
  type ExternalAccountDto,
} from '@/api'
import AiSettingsForm from '@/components/AiSettingsForm.vue'
import { COMMON_LANGS, langLabel } from '@/lib/langs'
import { PROFILE_TABS, resolveProfileTab, type ProfileTab } from '@/lib/profileTabs'
import { roleLabel } from '@/lib/states'
import { useAuthStore } from '@/stores/auth'

const auth = useAuthStore()
const $q = useQuasar()
const { locale, t } = useI18n()
const route = useRoute()
const router = useRouter()
const localizedLangLabel = (code: string) => langLabel(code, locale.value)

const desc = ref('')
const langs = ref<string[]>([])
const diffMode = ref<EntryDiffMode>('character_inline')
const previewTranslationDiff = ref(false)
const aiSourcePreference = ref<AiSourcePreference>('auto')
const saving = ref(false)
const keys = ref<ApiKeyDto[]>([])
const accounts = ref<ExternalAccountDto[]>([])
const aiSettings = ref<AiSettingsDto | null>(null)
const aiSaving = ref(false)
const availableKeyScopes = ref<ApiKeyScope[]>([])
const activeTab = ref<ProfileTab>('profile')

watch(
  () => route.query.tab,
  (requested) => {
    const resolved = resolveProfileTab(requested)
    activeTab.value = resolved
    if (requested !== resolved) void router.replace({ query: { ...route.query, tab: resolved } })
  },
  { immediate: true },
)

function selectTab(tab: ProfileTab) {
  activeTab.value = tab
  void router.replace({ query: { ...route.query, tab } })
}

onMounted(async () => {
  await auth.refreshMe()
  desc.value = auth.user?.description ?? ''
  langs.value = [...(auth.user?.translation_langs ?? [])]
  diffMode.value = auth.user?.entry_diff_mode ?? 'character_inline'
  previewTranslationDiff.value = auth.user?.preview_translation_diff ?? false
  aiSourcePreference.value = auth.user?.ai_source_preference ?? 'auto'
  try {
    const [loadedKeys, loadedAccounts, loadedAi, loadedScopes] = await Promise.all([
      usersApi.listApiKeys(),
      usersApi.accounts(),
      aiApi.getPersonalSettings(),
      usersApi.listApiKeyScopes(),
    ])
    keys.value = loadedKeys
    accounts.value = loadedAccounts
    aiSettings.value = loadedAi
    availableKeyScopes.value = loadedScopes
  } catch (e) {
    $q.notify({ type: 'negative', message: apiErrorMessage(e) })
  }
})

async function saveProfile() {
  saving.value = true
  try {
    await usersApi.updateMe({
      description: desc.value,
      translation_langs: langs.value,
      entry_diff_mode: diffMode.value,
      preview_translation_diff: previewTranslationDiff.value,
      ai_source_preference: aiSourcePreference.value,
    })
    await auth.refreshMe()
    $q.notify({ type: 'positive', message: t('profile.saved') })
  } catch (e) {
    $q.notify({ type: 'negative', message: apiErrorMessage(e, t('profile.saveFailed')) })
  } finally {
    saving.value = false
  }
}

async function savePersonalAi(request: AiSettingsWriteRequest) {
  aiSaving.value = true
  try {
    aiSettings.value = await aiApi.putPersonalSettings(request)
    $q.notify({ type: 'positive', message: t('profile.ai.saved') })
  } catch (error) {
    $q.notify({ type: 'negative', message: apiErrorMessage(error, t('profile.ai.saveFailed')) })
  } finally {
    aiSaving.value = false
  }
}

function deletePersonalAi() {
  $q.dialog({
    title: t('profile.ai.delete'),
    message: t('profile.ai.deleteConfirm'),
    cancel: true,
  }).onOk(async () => {
    aiSaving.value = true
    try {
      await aiApi.deletePersonalSettings()
      aiSettings.value = await aiApi.getPersonalSettings()
      $q.notify({ type: 'positive', message: t('profile.ai.deleted') })
    } catch (error) {
      $q.notify({ type: 'negative', message: apiErrorMessage(error) })
    } finally {
      aiSaving.value = false
    }
  })
}

/* —— 密码修改与持久提醒 —— */
const currentPassword = ref('')
const newPassword = ref('')
const confirmPassword = ref('')
const passwordSaving = ref(false)
const passwordFormReady = computed(
  () =>
    currentPassword.value.length > 0 &&
    newPassword.value.length >= 8 &&
    newPassword.value === confirmPassword.value,
)

async function changePassword() {
  if (newPassword.value !== confirmPassword.value) {
    $q.notify({ type: 'negative', message: t('profile.password.mismatch') })
    return
  }
  passwordSaving.value = true
  try {
    await usersApi.changePassword({
      current_password: currentPassword.value,
      new_password: newPassword.value,
    })
    await auth.refreshMe()
    $q.notify({ type: 'positive', message: t('profile.password.changed') })
  } catch (error) {
    $q.notify({
      type: 'negative',
      message: apiErrorMessage(error, t('profile.password.changeFailed')),
    })
  } finally {
    currentPassword.value = ''
    newPassword.value = ''
    confirmPassword.value = ''
    passwordSaving.value = false
  }
}

/* —— API Key —— */
const showCreate = ref(false)
const newKeyName = ref('')
const newKeyScopes = ref<ApiKeyScope[]>(['all'])
const createdKey = ref<string | null>(null)
const editingKey = ref<ApiKeyDto | null>(null)
const editKeyName = ref('')
const editKeyScopes = ref<ApiKeyScope[]>([])
const keySaving = ref(false)

const scopeOptions = computed(() =>
  availableKeyScopes.value.map((scope) => ({
    value: scope,
    label: t(`profile.apiKeys.scopes.${scope.replace(':', '_')}`),
  })),
)

function normalizeScopeSelection(next: ApiKeyScope[], previous: ApiKeyScope[]): ApiKeyScope[] {
  if (next.includes('all') && !previous.includes('all')) return ['all']
  if (previous.includes('all') && next.length > 1) return next.filter((scope) => scope !== 'all')
  return next
}

function updateNewKeyScopes(next: ApiKeyScope[]) {
  newKeyScopes.value = normalizeScopeSelection(next, newKeyScopes.value)
}

function updateEditKeyScopes(next: ApiKeyScope[]) {
  editKeyScopes.value = normalizeScopeSelection(next, editKeyScopes.value)
}

function openCreate() {
  newKeyName.value = ''
  newKeyScopes.value = ['all']
  createdKey.value = null
  showCreate.value = true
}
async function createKey() {
  if (!newKeyName.value.trim() || !newKeyScopes.value.length) return
  keySaving.value = true
  try {
    const k = await usersApi.createApiKey(newKeyName.value.trim(), newKeyScopes.value)
    createdKey.value = k.key
    keys.value.unshift({
      id: k.id,
      name: k.name,
      prefix: k.prefix,
      created_at: k.created_at,
      last_used_at: null,
      scopes: k.scopes,
    })
  } catch (e) {
    $q.notify({ type: 'negative', message: apiErrorMessage(e, t('profile.apiKeys.createFailed')) })
  } finally {
    keySaving.value = false
  }
}

function openEditKey(key: ApiKeyDto) {
  editingKey.value = key
  editKeyName.value = key.name
  editKeyScopes.value = [...key.scopes]
}

async function saveKey() {
  const key = editingKey.value
  if (!key || !editKeyName.value.trim() || !editKeyScopes.value.length) return
  keySaving.value = true
  try {
    const updated = await usersApi.updateApiKey(
      key.id,
      editKeyName.value.trim(),
      editKeyScopes.value,
    )
    keys.value = keys.value.map((item) => (item.id === updated.id ? updated : item))
    editingKey.value = null
    $q.notify({ type: 'positive', message: t('profile.apiKeys.updated') })
  } catch (error) {
    $q.notify({ type: 'negative', message: apiErrorMessage(error) })
  } finally {
    keySaving.value = false
  }
}
function copyKey() {
  if (createdKey.value) {
    navigator.clipboard?.writeText(createdKey.value)
    $q.notify({ type: 'positive', message: t('common.copied'), timeout: 800 })
  }
}
async function revokeKey(id: number) {
  try {
    await usersApi.revokeApiKey(id)
    keys.value = keys.value.filter((k) => k.id !== id)
  } catch (e) {
    $q.notify({ type: 'negative', message: apiErrorMessage(e) })
  }
}
</script>

<template>
  <q-page class="prts-container prts-container--narrow">
    <div class="prts-label">// OPERATOR</div>
    <h1 class="prts-h1 q-mb-lg">{{ t('profile.title') }}</h1>

    <q-tabs
      :model-value="activeTab"
      class="profile-tabs q-mb-lg"
      dense
      no-caps
      outside-arrows
      mobile-arrows
      align="left"
      active-color="primary"
      indicator-color="primary"
      @update:model-value="selectTab($event as ProfileTab)"
    >
      <q-tab v-for="tab in PROFILE_TABS" :key="tab" :name="tab" :label="t(`profile.tabs.${tab}`)" />
    </q-tabs>

    <template v-if="activeTab === 'profile'">
      <q-card flat bordered class="q-pa-lg q-mb-lg">
        <div class="profile-summary row items-center q-gutter-md">
          <q-avatar size="64px" color="primary" text-color="dark">
            <img v-if="auth.user?.avatar_url" :src="auth.user.avatar_url" alt="" />
            <span v-else>{{ auth.user?.username?.slice(0, 2).toUpperCase() }}</span>
          </q-avatar>
          <div>
            <div class="prts-h2">{{ auth.user?.username }}</div>
            <div class="prts-mono prts-dim q-mt-xs" style="font-size: 12px">
              UID {{ auth.user?.id }} ·
              <q-badge
                v-if="auth.role"
                color="primary"
                text-color="dark"
                :label="roleLabel(auth.role, t)"
              />
            </div>
          </div>
          <q-space />
          <div class="text-center">
            <div class="prts-display text-accent" style="font-size: 28px">
              {{
                ((auth.user?.cp_tenths ?? 0) / 10).toLocaleString(undefined, {
                  maximumFractionDigits: 1,
                })
              }}
            </div>
            <div class="prts-label">{{ t('profile.cp') }}</div>
          </div>
        </div>

        <q-separator class="q-my-lg" />

        <div class="column q-gutter-md">
          <div class="fld">
            <div class="prts-label q-mb-xs">{{ t('profile.description') }}</div>
            <q-input v-model="desc" outlined dense type="textarea" autogrow :disable="saving" />
          </div>
          <div class="fld">
            <q-toggle
              v-model="previewTranslationDiff"
              :label="t('profile.previewTranslationDiff')"
              :disable="saving"
            />
            <div class="prts-dim">{{ t('profile.previewTranslationDiffHint') }}</div>
          </div>
          <div class="fld">
            <div class="prts-label q-mb-xs">{{ t('profile.ai.sourcePreference') }}</div>
            <q-select
              v-model="aiSourcePreference"
              outlined
              dense
              emit-value
              map-options
              :options="[
                { label: t('profile.ai.sources.auto'), value: 'auto' },
                { label: t('profile.ai.sources.personal'), value: 'personal' },
                { label: t('profile.ai.sources.project'), value: 'project' },
              ]"
              :hint="t('profile.ai.sourcePreferenceHint')"
              :disable="saving"
            />
          </div>
          <div class="fld">
            <div class="prts-label q-mb-xs">{{ t('profile.entryDiffMode') }}</div>
            <q-select
              v-model="diffMode"
              outlined
              dense
              emit-value
              map-options
              :options="[
                { label: t('profile.diffModes.character'), value: 'character_inline' },
                { label: t('profile.diffModes.word'), value: 'word_inline' },
                { label: t('profile.diffModes.sideBySide'), value: 'side_by_side' },
              ]"
              :hint="t('profile.entryDiffModeHint')"
              :disable="saving"
            />
          </div>
          <div class="fld">
            <div class="prts-label q-mb-xs">{{ t('profile.translationLanguages') }}</div>
            <q-select
              v-model="langs"
              outlined
              dense
              multiple
              use-chips
              use-input
              input-debounce="0"
              new-value-mode="add-unique"
              :options="COMMON_LANGS"
              :option-label="localizedLangLabel"
              :disable="saving"
            />
          </div>
          <div>
            <q-btn
              unelevated
              no-caps
              color="primary"
              text-color="dark"
              :label="t('profile.save')"
              :loading="saving"
              @click="saveProfile"
            />
          </div>
        </div>
      </q-card>
    </template>

    <template v-if="activeTab === 'ai'">
      <div class="prts-label q-mb-sm">{{ t('profile.ai.heading') }}</div>
      <q-card flat bordered class="q-pa-lg q-mb-lg">
        <AiSettingsForm
          :settings="aiSettings"
          :loading="aiSaving"
          scope="personal"
          @save="savePersonalAi"
          @delete="deletePersonalAi"
        />
      </q-card>
    </template>

    <template v-if="activeTab === 'security'">
      <div class="prts-label q-mb-sm">{{ t('profile.password.title') }}</div>
      <q-card flat bordered class="q-pa-lg q-mb-lg">
        <div class="prts-dim q-mb-md">{{ t('profile.password.description') }}</div>
        <q-banner
          v-if="auth.passwordChangeRequired"
          dense
          rounded
          class="bg-warning text-dark q-mb-md"
        >
          {{ t('profile.password.required') }}
        </q-banner>
        <div class="column q-gutter-md">
          <q-input
            v-model="currentPassword"
            outlined
            dense
            type="password"
            autocomplete="current-password"
            :label="t('profile.password.current')"
            :disable="passwordSaving"
          />
          <q-input
            v-model="newPassword"
            outlined
            dense
            type="password"
            autocomplete="new-password"
            :label="t('profile.password.new')"
            :hint="t('profile.password.hint')"
            :disable="passwordSaving"
          />
          <q-input
            v-model="confirmPassword"
            outlined
            dense
            type="password"
            autocomplete="new-password"
            :label="t('profile.password.confirm')"
            :disable="passwordSaving"
          />
          <div>
            <q-btn
              unelevated
              no-caps
              color="primary"
              text-color="dark"
              :label="t('profile.password.submit')"
              :loading="passwordSaving"
              :disable="!passwordFormReady"
              @click="changePassword"
            />
          </div>
        </div>
      </q-card>

      <!-- linked accounts -->
      <div class="prts-label q-mb-sm">{{ t('profile.linkedAccounts') }}</div>
      <q-card flat bordered class="q-mb-lg">
        <q-list v-if="accounts.length" separator>
          <q-item v-for="a in accounts" :key="a.provider + a.external_id">
            <q-item-section avatar
              ><q-icon name="mdi-link-variant" color="primary"
            /></q-item-section>
            <q-item-section>
              <q-item-label>{{ a.provider }}</q-item-label>
              <q-item-label caption class="prts-mono">{{ a.external_id }}</q-item-label>
            </q-item-section>
          </q-item>
        </q-list>
        <div v-else class="prts-empty" style="padding: 30px">
          {{ t('profile.noLinkedAccounts') }}
        </div>
      </q-card>
    </template>

    <!-- api keys -->
    <template v-if="activeTab === 'api_keys'">
      <div class="row items-center q-mb-sm">
        <div class="prts-label">API KEY</div>
        <q-space />
        <q-btn
          flat
          dense
          no-caps
          size="sm"
          icon="mdi-plus"
          :label="t('profile.apiKeys.new')"
          @click="openCreate"
        />
      </div>
      <q-card flat bordered>
        <q-list v-if="keys.length" separator>
          <q-item v-for="k in keys" :key="k.id">
            <q-item-section>
              <q-item-label>{{ k.name }}</q-item-label>
              <q-item-label caption class="prts-mono"
                >{{ k.prefix }}··· · {{ t('profile.apiKeys.createdAt') }}
                {{ new Date(k.created_at).toLocaleDateString() }}</q-item-label
              >
              <div class="row q-gutter-xs q-mt-xs">
                <q-chip v-for="scope in k.scopes" :key="scope" dense square outline color="primary">
                  {{ t(`profile.apiKeys.scopes.${scope.replace(':', '_')}`) }}
                </q-chip>
              </div>
            </q-item-section>
            <q-item-section side class="row no-wrap">
              <q-btn
                flat
                round
                dense
                size="sm"
                icon="mdi-pencil-outline"
                :aria-label="t('profile.apiKeys.edit')"
                @click="openEditKey(k)"
              />
              <q-btn
                flat
                round
                dense
                size="sm"
                icon="mdi-delete-outline"
                color="negative"
                @click="revokeKey(k.id)"
              />
            </q-item-section>
          </q-item>
        </q-list>
        <div v-else class="prts-empty" style="padding: 30px">
          {{ t('profile.apiKeys.empty') }}
        </div>
      </q-card>
    </template>

    <q-dialog v-model="showCreate">
      <q-card style="width: 440px; max-width: 92vw">
        <q-card-section
          ><div class="prts-h2">{{ t('profile.apiKeys.new') }}</div></q-card-section
        >
        <q-card-section>
          <template v-if="!createdKey">
            <q-input
              v-model="newKeyName"
              outlined
              dense
              :label="t('profile.apiKeys.name')"
              autofocus
              @keyup.enter="createKey"
            />
            <div class="prts-label q-mt-md q-mb-xs">{{ t('profile.apiKeys.permissions') }}</div>
            <q-option-group
              :model-value="newKeyScopes"
              :options="scopeOptions"
              type="checkbox"
              color="primary"
              @update:model-value="updateNewKeyScopes($event as ApiKeyScope[])"
            />
            <div class="prts-dim q-mt-sm">{{ t('profile.apiKeys.permissionsHint') }}</div>
          </template>
          <template v-else>
            <div class="prts-dim q-mb-sm" style="font-size: 13px">
              {{ t('profile.apiKeys.copyNow') }}
            </div>
            <q-input :model-value="createdKey" readonly outlined dense input-class="prts-mono">
              <template #append
                ><q-btn flat dense round icon="mdi-content-copy" @click="copyKey"
              /></template>
            </q-input>
          </template>
        </q-card-section>
        <q-card-actions align="right">
          <q-btn v-if="!createdKey" v-close-popup flat no-caps :label="t('common.cancel')" />
          <q-btn
            v-if="!createdKey"
            unelevated
            no-caps
            color="primary"
            text-color="dark"
            :label="t('common.create')"
            :loading="keySaving"
            :disable="!newKeyName.trim() || !newKeyScopes.length"
            @click="createKey"
          />
          <q-btn v-else v-close-popup flat no-caps color="primary" :label="t('common.done')" />
        </q-card-actions>
      </q-card>
    </q-dialog>

    <q-dialog
      :model-value="editingKey !== null"
      @update:model-value="!$event && (editingKey = null)"
    >
      <q-card style="width: 480px; max-width: 92vw">
        <q-card-section
          ><div class="prts-h2">{{ t('profile.apiKeys.edit') }}</div></q-card-section
        >
        <q-card-section>
          <q-input v-model="editKeyName" outlined dense :label="t('profile.apiKeys.name')" />
          <div class="prts-label q-mt-md q-mb-xs">{{ t('profile.apiKeys.permissions') }}</div>
          <q-option-group
            :model-value="editKeyScopes"
            :options="scopeOptions"
            type="checkbox"
            color="primary"
            @update:model-value="updateEditKeyScopes($event as ApiKeyScope[])"
          />
          <div class="prts-dim q-mt-sm">{{ t('profile.apiKeys.permissionsHint') }}</div>
        </q-card-section>
        <q-card-actions align="right">
          <q-btn flat no-caps :label="t('common.cancel')" @click="editingKey = null" />
          <q-btn
            unelevated
            no-caps
            color="primary"
            text-color="dark"
            :label="t('common.save')"
            :loading="keySaving"
            :disable="!editKeyName.trim() || !editKeyScopes.length"
            @click="saveKey"
          />
        </q-card-actions>
      </q-card>
    </q-dialog>
  </q-page>
</template>

<style scoped>
.profile-summary > div {
  min-width: 0;
}

.profile-tabs {
  border-bottom: 1px solid var(--prts-border-soft);
}

.profile-summary .prts-h2 {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

@media (max-width: 390px) {
  .profile-summary {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr);
    gap: 12px;
  }

  .profile-summary > .q-space {
    display: none;
  }

  .profile-summary > .text-center {
    grid-column: 1 / -1;
    justify-self: start;
    text-align: left;
  }
}
</style>
