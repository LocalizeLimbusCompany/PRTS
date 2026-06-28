<script setup lang="ts">
import { ref } from 'vue'
import { useRouter } from 'vue-router'
import { useQuasar } from 'quasar'

import { apiErrorMessage } from '@/api'
import { useAuthStore } from '@/stores/auth'

const auth = useAuthStore()
const router = useRouter()
const $q = useQuasar()

const username = ref('')
const email = ref('')
const password = ref('')
const loading = ref(false)

async function submit() {
  loading.value = true
  try {
    await auth.register(username.value.trim(), password.value, email.value.trim() || undefined)
    $q.notify({ type: 'positive', message: '注册成功，已登录' })
    router.push('/projects')
  } catch (e) {
    $q.notify({
      type: 'negative',
      message: apiErrorMessage(e, '注册失败（用户名/邮箱可能已被占用）'),
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
      <div class="prts-h1 q-mt-xs q-mb-lg">注册账号</div>

      <q-form class="column q-gutter-md" @submit.prevent="submit">
        <q-input
          v-model="username"
          outlined
          dense
          label="用户名"
          hint="3–32 字符"
          :disable="loading"
          :rules="[(v) => (v.trim().length >= 3 && v.trim().length <= 32) || '需 3–32 字符']"
          hide-bottom-space
        />
        <q-input
          v-model="email"
          outlined
          dense
          type="email"
          label="邮箱（可选）"
          :disable="loading"
        />
        <q-input
          v-model="password"
          outlined
          dense
          type="password"
          label="密码"
          hint="至少 8 位"
          autocomplete="new-password"
          :disable="loading"
          :rules="[(v) => v.length >= 8 || '至少 8 位']"
          hide-bottom-space
        />
        <q-btn
          type="submit"
          unelevated
          no-caps
          color="primary"
          text-color="dark"
          :loading="loading"
          label="注册"
        />
      </q-form>

      <div class="auth-foot">已有账号？<router-link to="/login">登录</router-link></div>
    </q-card>
  </q-page>
</template>
