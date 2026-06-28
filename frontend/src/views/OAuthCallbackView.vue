<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'

import { useAuthStore } from '@/stores/auth'

const route = useRoute()
const router = useRouter()
const auth = useAuthStore()
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
      error.value = '令牌无效'
    }
  } else {
    error.value = '回调缺少令牌'
  }
})
</script>

<template>
  <q-page class="auth-page">
    <q-card class="auth-card column items-center" style="text-align: center">
      <template v-if="error">
        <q-icon name="error_outline" size="40px" color="negative" />
        <div class="prts-h2 q-mt-md">登录失败</div>
        <div class="prts-dim q-mt-xs">{{ error }}</div>
        <q-btn flat no-caps color="primary" to="/login" label="返回登录" class="q-mt-md" />
      </template>
      <template v-else>
        <q-spinner-gears size="42px" color="primary" />
        <div class="prts-mono prts-dim q-mt-md">正在完成 ZOOT 登录…</div>
      </template>
    </q-card>
  </q-page>
</template>
