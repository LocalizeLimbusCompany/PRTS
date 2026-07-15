<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useQuasar } from 'quasar'
import { useI18n } from 'vue-i18n'

import { apiErrorMessage, authApi, type AuthConfigDto } from '@/api'
import { useAuthStore } from '@/stores/auth'

const auth = useAuthStore()
const router = useRouter()
const route = useRoute()
const $q = useQuasar()
const { t } = useI18n()

const username = ref('')
const password = ref('')
const loading = ref(false)
const config = ref<AuthConfigDto | null>(null)
const configLoading = ref(true)
const configFailed = ref(false)
const passwordLoginEnabled = computed(() => config.value?.password_login_enabled === true)
const passwordRegistrationEnabled = computed(
  () => config.value?.password_registration_enabled === true,
)
const zootEnabled = computed(() => config.value?.oauth_providers.includes('zoot') === true)

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
    await auth.login(username.value.trim(), password.value)
    router.push((route.query.redirect as string) || '/projects')
  } catch (e) {
    $q.notify({ type: 'negative', message: apiErrorMessage(e, t('auth.login.failed')) })
  } finally {
    loading.value = false
  }
}

async function zoot() {
  try {
    const { authorize_url } = await authApi.oauthStart('zoot')
    window.location.href = authorize_url
  } catch (e) {
    $q.notify({ type: 'negative', message: apiErrorMessage(e, t('auth.login.zootUnavailable')) })
  }
}
</script>

<template>
  <q-page class="auth-page">
    <q-card class="auth-card">
      <div class="prts-label">// SIGN IN</div>
      <div class="prts-h1 q-mt-xs q-mb-lg">{{ t('auth.login.title') }}</div>

      <div v-if="configLoading" class="row justify-center q-py-xl">
        <q-spinner color="primary" size="32px" />
      </div>

      <q-banner v-else-if="configFailed" rounded class="bg-negative text-white">
        {{ t('auth.configLoadFailed') }}
        <template #action>
          <q-btn flat no-caps color="white" :label="t('auth.retry')" @click="loadConfig" />
        </template>
      </q-banner>

      <template v-else>
        <q-form v-if="passwordLoginEnabled" class="column q-gutter-md" @submit.prevent="submit">
          <q-input
            v-model="username"
            outlined
            dense
            :label="t('auth.login.username')"
            autocomplete="username"
            :disable="loading"
            :rules="[(v) => !!v || t('auth.required')]"
            hide-bottom-space
          />
          <q-input
            v-model="password"
            outlined
            dense
            type="password"
            :label="t('auth.login.password')"
            autocomplete="current-password"
            :disable="loading"
            :rules="[(v) => !!v || t('auth.required')]"
            hide-bottom-space
          />
          <q-btn
            type="submit"
            unelevated
            no-caps
            color="primary"
            text-color="dark"
            :loading="loading"
            :label="t('auth.login.submit')"
          />
        </q-form>

        <div v-if="passwordLoginEnabled && zootEnabled" class="auth-separator q-my-md">
          <q-separator />
          <span>{{ t('auth.login.or') }}</span>
          <q-separator />
        </div>

        <q-btn
          v-if="zootEnabled"
          type="button"
          outline
          no-caps
          color="primary"
          class="full-width"
          :label="t('auth.login.zoot')"
          @click="zoot"
        />

        <q-banner v-if="!passwordLoginEnabled && !zootEnabled" rounded class="bg-warning text-dark">
          {{ t('auth.login.noMethods') }}
        </q-banner>

        <div v-if="passwordRegistrationEnabled" class="auth-foot">
          {{ t('auth.login.noAccount')
          }}<router-link to="/register">{{ t('auth.login.registerLink') }}</router-link>
        </div>
      </template>
    </q-card>
  </q-page>
</template>

<style scoped>
.auth-separator {
  display: grid;
  grid-template-columns: 1fr auto 1fr;
  align-items: center;
  gap: 12px;
  color: var(--prts-muted);
  font-size: 12px;
}
</style>
