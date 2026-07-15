<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'

import { useAuthStore } from '@/stores/auth'

const route = useRoute()
const router = useRouter()
const auth = useAuthStore()
const { t } = useI18n()
const error = ref('')

onMounted(async () => {
  if (route.query.error) {
    error.value = String(route.query.error)
    return
  }
  const access = route.query.access_token as string | undefined
  const refresh = route.query.refresh_token as string | undefined
  if (access && refresh) {
    try {
      await auth.applyTokens(access, refresh)
      router.replace('/projects')
    } catch {
      error.value = t('auth.oauth.invalidToken')
    }
  } else {
    error.value = t('auth.oauth.missingToken')
  }
})
</script>

<template>
  <q-page class="auth-page">
    <q-card class="auth-card column items-center" style="text-align: center">
      <template v-if="error">
        <q-icon name="mdi-alert-circle-outline" size="40px" color="negative" />
        <div class="prts-h2 q-mt-md">{{ t('auth.oauth.failed') }}</div>
        <div class="prts-dim q-mt-xs">{{ error }}</div>
        <q-btn
          flat
          no-caps
          color="primary"
          to="/login"
          :label="t('auth.oauth.backToLogin')"
          class="q-mt-md"
        />
      </template>
      <template v-else>
        <q-spinner-gears size="42px" color="primary" />
        <div class="prts-mono prts-dim q-mt-md">{{ t('auth.oauth.completing') }}</div>
      </template>
    </q-card>
  </q-page>
</template>
