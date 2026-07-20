<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useQuasar } from 'quasar'
import { useI18n } from 'vue-i18n'

import {
  apiErrorMessage,
  usersApi,
  type ApiKeyDto,
  type EntryDiffMode,
  type ExternalAccountDto,
} from '@/api'
import { COMMON_LANGS, langLabel } from '@/lib/langs'
import { roleLabel } from '@/lib/states'
import { useAuthStore } from '@/stores/auth'

const auth = useAuthStore()
const $q = useQuasar()
const { locale, t } = useI18n()
const localizedLangLabel = (code: string) => langLabel(code, locale.value)

const desc = ref('')
const langs = ref<string[]>([])
const diffMode = ref<EntryDiffMode>('word_inline')
const saving = ref(false)
const keys = ref<ApiKeyDto[]>([])
const accounts = ref<ExternalAccountDto[]>([])

onMounted(async () => {
  await auth.refreshMe()
  desc.value = auth.user?.description ?? ''
  langs.value = [...(auth.user?.translation_langs ?? [])]
  diffMode.value = auth.user?.entry_diff_mode ?? 'word_inline'
  try {
    keys.value = await usersApi.listApiKeys()
    accounts.value = await usersApi.accounts()
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
    })
    await auth.refreshMe()
    $q.notify({ type: 'positive', message: t('profile.saved') })
  } catch (e) {
    $q.notify({ type: 'negative', message: apiErrorMessage(e, t('profile.saveFailed')) })
  } finally {
    saving.value = false
  }
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
const createdKey = ref<string | null>(null)

function openCreate() {
  newKeyName.value = ''
  createdKey.value = null
  showCreate.value = true
}
async function createKey() {
  if (!newKeyName.value.trim()) return
  try {
    const k = await usersApi.createApiKey(newKeyName.value.trim())
    createdKey.value = k.key
    keys.value.unshift({
      id: k.id,
      name: k.name,
      prefix: k.prefix,
      created_at: k.created_at,
      last_used_at: null,
    })
  } catch (e) {
    $q.notify({ type: 'negative', message: apiErrorMessage(e, t('profile.apiKeys.createFailed')) })
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

    <q-card flat bordered class="q-pa-lg q-mb-lg">
      <div class="row items-center q-gutter-md">
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
          <q-item-section avatar><q-icon name="mdi-link-variant" color="primary" /></q-item-section>
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

    <!-- api keys -->
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
          </q-item-section>
          <q-item-section side>
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
            @click="createKey"
          />
          <q-btn v-else v-close-popup flat no-caps color="primary" :label="t('common.done')" />
        </q-card-actions>
      </q-card>
    </q-dialog>
  </q-page>
</template>
