<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { useQuasar } from 'quasar'

import { adminApi, apiErrorMessage } from '@/api'
import { useAuthStore } from '@/stores/auth'

const auth = useAuthStore()
const $q = useQuasar()

const oauthOnly = ref(false)
const registrationOpen = ref(true)
const requireEmail = ref(false)
const savingSettings = ref(false)

async function loadSettings() {
  try {
    const s = await adminApi.getSettings()
    oauthOnly.value = s['auth.oauth_only'] === true
    registrationOpen.value = s['auth.registration_open'] !== false
    requireEmail.value = s['auth.require_email_verification'] === true
  } catch (e) {
    $q.notify({ type: 'negative', message: apiErrorMessage(e) })
  }
}
onMounted(loadSettings)

async function saveSettings() {
  savingSettings.value = true
  try {
    await adminApi.updateSettings({
      'auth.oauth_only': oauthOnly.value,
      'auth.registration_open': registrationOpen.value,
      'auth.require_email_verification': requireEmail.value,
    })
    $q.notify({ type: 'positive', message: '设置已保存' })
  } catch (e) {
    $q.notify({ type: 'negative', message: apiErrorMessage(e, '保存失败') })
  } finally {
    savingSettings.value = false
  }
}

/* —— 角色任免（仅总管理员）—— */
const grantUserId = ref<number | null>(null)
const grantRoleVal = ref<string | null>('maintainer')
const granting = ref(false)
const roleOptions = [
  { label: '总管理员', value: 'super_admin' },
  { label: '管理员', value: 'admin' },
  { label: '维护者', value: 'maintainer' },
  { label: '普通用户（移除角色）', value: null },
]
async function doGrant() {
  if (grantUserId.value == null) return
  granting.value = true
  try {
    await adminApi.grantRole(grantUserId.value, grantRoleVal.value)
    $q.notify({ type: 'positive', message: '角色已更新' })
    grantUserId.value = null
  } catch (e) {
    $q.notify({ type: 'negative', message: apiErrorMessage(e, '操作失败') })
  } finally {
    granting.value = false
  }
}
</script>

<template>
  <q-page class="prts-container prts-container--narrow">
    <div class="prts-label">// ADMIN</div>
    <h1 class="prts-h1 q-mb-lg">管理后台</h1>

    <div class="prts-label q-mb-sm">平台设置</div>
    <q-card flat bordered class="q-pa-md q-mb-lg">
      <q-toggle v-model="registrationOpen" label="开放注册" :disable="savingSettings" />
      <div class="prts-dim q-mb-md" style="font-size: 12px; margin-left: 52px">
        关闭后新用户无法自助注册。
      </div>
      <q-toggle
        v-model="oauthOnly"
        label="仅 OAuth 登录（禁用账号密码）"
        :disable="savingSettings"
      />
      <div class="prts-dim q-mb-md" style="font-size: 12px; margin-left: 52px">
        开启后仅允许 ZOOT 等 OAuth 登录。
      </div>
      <q-toggle v-model="requireEmail" label="要求邮箱验证" :disable="savingSettings" />
      <div class="prts-dim" style="font-size: 12px; margin-left: 52px">
        需配置 SMTP 后实际生效（投递功能后续接入）。
      </div>
      <div class="q-mt-md">
        <q-btn
          unelevated
          no-caps
          color="primary"
          text-color="dark"
          label="保存设置"
          :loading="savingSettings"
          @click="saveSettings"
        />
      </div>
    </q-card>

    <template v-if="auth.isSuperAdmin">
      <div class="prts-label q-mb-sm">角色任免</div>
      <q-card flat bordered class="q-pa-md">
        <div class="prts-dim q-mb-md" style="font-size: 13px">
          按用户 UID 设置平台角色（仅总管理员）。
        </div>
        <div class="row q-col-gutter-md items-center">
          <div class="col-12 col-sm-4">
            <q-input v-model.number="grantUserId" outlined dense type="number" label="用户 UID" />
          </div>
          <div class="col-12 col-sm-5">
            <q-select
              v-model="grantRoleVal"
              outlined
              dense
              :options="roleOptions"
              emit-value
              map-options
              label="角色"
            />
          </div>
          <div class="col-12 col-sm-3">
            <q-btn
              unelevated
              no-caps
              color="primary"
              text-color="dark"
              class="full-width"
              label="应用"
              :loading="granting"
              @click="doGrant"
            />
          </div>
        </div>
      </q-card>
    </template>
  </q-page>
</template>
