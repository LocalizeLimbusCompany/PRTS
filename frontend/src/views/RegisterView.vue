<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { useQuasar } from 'quasar'
import { useI18n } from 'vue-i18n'

import { apiErrorMessage, authApi, type AuthConfigDto } from '@/api'
import { useAuthStore } from '@/stores/auth'

const auth = useAuthStore()
const router = useRouter()
const $q = useQuasar()
const { t } = useI18n()

const username = ref('')
const email = ref('')
const password = ref('')
const loading = ref(false)
const config = ref<AuthConfigDto | null>(null)
const configLoading = ref(true)
const configFailed = ref(false)

async function loadConfig() {
  configLoading.value = true
  configFailed.value = false
  try {
    config.value = await authApi.config()
  } catch {
    config.value = null
    configFailed.value = true
  } finally {
    configLoading.value = false
  }
}

onMounted(loadConfig)

async function submit() {
  loading.value = true
  try {
    await auth.register(username.value.trim(), password.value, email.value.trim() || undefined)
    $q.notify({ type: 'positive', message: t('auth.register.success') })
    router.push('/projects')
  } catch (e) {
    $q.notify({
      type: 'negative',
      message: apiErrorMessage(e, t('auth.register.failed')),
    })
  } finally {
    loading.value = false
  }
}
</script>

<template>
  <q-page class="auth-page">
    <q-card class="auth-card">
      <div class="prts-label">// REGISTER</div>
      <div class="prts-h1 q-mt-xs q-mb-lg">{{ t('auth.register.title') }}</div>

      <div v-if="configLoading" class="row justify-center q-py-xl">
        <q-spinner color="primary" size="32px" />
      </div>

      <q-banner v-else-if="configFailed" rounded class="bg-negative text-white">
        {{ t('auth.configLoadFailed') }}
        <template #action>
          <q-btn flat no-caps color="white" :label="t('auth.retry')" @click="loadConfig" />
        </template>
      </q-banner>

      <q-form
        v-else-if="config?.password_registration_enabled"
        class="column q-gutter-md"
        @submit.prevent="submit"
      >
        <q-input
          v-model="username"
          outlined
          dense
          :label="t('auth.register.username')"
          :hint="t('auth.register.usernameHint')"
          :disable="loading"
          :rules="[
            (v) =>
              (v.trim().length >= 3 && v.trim().length <= 32) || t('auth.register.usernameHint'),
          ]"
          hide-bottom-space
        />
        <q-input
          v-model="email"
          outlined
          dense
          type="email"
          :label="t('auth.register.email')"
          :disable="loading"
        />
        <q-input
          v-model="password"
          outlined
          dense
          type="password"
          :label="t('auth.register.password')"
          :hint="t('auth.register.passwordHint')"
          autocomplete="new-password"
          :disable="loading"
          :rules="[(v) => v.length >= 8 || t('auth.register.passwordHint')]"
          hide-bottom-space
        />
        <q-btn
          type="submit"
          unelevated
          no-caps
          color="primary"
          text-color="dark"
          :loading="loading"
          :label="t('auth.register.submit')"
        />
      </q-form>

      <q-banner v-else rounded class="bg-warning text-dark">
        {{ t('auth.register.disabled') }}
      </q-banner>

      <div v-if="!configLoading && !configFailed" class="auth-foot">
        {{ t('auth.register.hasAccount')
        }}<router-link to="/login">{{ t('auth.register.loginLink') }}</router-link>
      </div>
    </q-card>
  </q-page>
</template>
