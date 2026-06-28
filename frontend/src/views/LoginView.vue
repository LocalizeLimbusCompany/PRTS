<script setup lang="ts">
import { ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useQuasar } from 'quasar'

import { apiErrorMessage, authApi } from '@/api'
import { useAuthStore } from '@/stores/auth'

const auth = useAuthStore()
const router = useRouter()
const route = useRoute()
const $q = useQuasar()

const username = ref('')
const password = ref('')
const loading = ref(false)

async function submit() {
  loading.value = true
  try {
    await auth.login(username.value.trim(), password.value)
    router.push((route.query.redirect as string) || '/projects')
  } catch (e) {
    $q.notify({ type: 'negative', message: apiErrorMessage(e, '登录失败') })
  } finally {
    loading.value = false
  }
}

async function zoot() {
  try {
    const { authorize_url } = await authApi.oauthStart('zoot')
    window.location.href = authorize_url
  } catch (e) {
    $q.notify({ type: 'negative', message: apiErrorMessage(e, 'ZOOT 登录暂不可用') })
  }
}
</script>

<template>
  <q-page class="auth-page">
    <q-card class="auth-card">
      <div class="prts-label">// SIGN IN</div>
      <div class="prts-h1 q-mt-xs q-mb-lg">登录终端</div>

      <q-form class="column q-gutter-md" @submit.prevent="submit">
        <q-input
          v-model="username"
          outlined
          dense
          label="用户名 / 邮箱"
          autocomplete="username"
          :disable="loading"
          :rules="[(v) => !!v || '必填']"
          hide-bottom-space
        />
        <q-input
          v-model="password"
          outlined
          dense
          type="password"
          label="密码"
          autocomplete="current-password"
          :disable="loading"
          :rules="[(v) => !!v || '必填']"
          hide-bottom-space
        />
        <q-btn
          type="submit"
          unelevated
          no-caps
          color="primary"
          text-color="dark"
          :loading="loading"
          label="登录"
        />
        <q-btn
          type="button"
          outline
          no-caps
          color="primary"
          label="用 ZOOT 账号登录"
          @click="zoot"
        />
      </q-form>

      <div class="auth-foot">还没有账号？<router-link to="/register">注册</router-link></div>
    </q-card>
  </q-page>
</template>
