<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { useQuasar } from 'quasar'
import { useI18n } from 'vue-i18n'

import { adminApi, adminSearchApi, apiErrorMessage } from '@/api'
import type { SearchSettingsDto } from '@/api'
import { useAuthStore } from '@/stores/auth'

const auth = useAuthStore()
const $q = useQuasar()
const { t } = useI18n()

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

/* —— 搜索 / 向量化设置 —— */
const searchSettings = ref<SearchSettingsDto>({
  embedding_enabled: false,
  embedding_model: '',
  embedding_base_url: '',
  embedding_batch: 4,
  tm_enabled: false,
  tm_min_similarity: 0.7,
  tm_top_n: 3,
  embedding_key_present: false,
})
const savingSearch = ref(false)

async function loadSearchSettings() {
  try {
    searchSettings.value = await adminSearchApi.get()
  } catch (e) {
    $q.notify({ type: 'negative', message: apiErrorMessage(e) })
  }
}
onMounted(loadSearchSettings)

async function saveSearchSettings() {
  savingSearch.value = true
  try {
    const result = await adminSearchApi.put({
      embedding_enabled: searchSettings.value.embedding_enabled,
      embedding_model: searchSettings.value.embedding_model,
      embedding_base_url: searchSettings.value.embedding_base_url,
      embedding_batch: searchSettings.value.embedding_batch,
      tm_enabled: searchSettings.value.tm_enabled,
      tm_min_similarity: searchSettings.value.tm_min_similarity,
      tm_top_n: searchSettings.value.tm_top_n,
    })
    searchSettings.value = result
    $q.notify({ type: 'positive', message: t('admin.search.saved') })
  } catch (e) {
    $q.notify({ type: 'negative', message: apiErrorMessage(e, t('admin.search.saveFailed')) })
  } finally {
    savingSearch.value = false
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

    <!-- 搜索 / 向量化设置 -->
    <div class="prts-label q-mb-sm">{{ t('admin.search.title') }}</div>
    <q-card flat bordered class="q-pa-md q-mb-lg">
      <div class="prts-dim q-mb-md" style="font-size: 13px">{{ t('admin.search.subtitle') }}</div>

      <!-- 向量化开关 -->
      <q-toggle
        v-model="searchSettings.embedding_enabled"
        :label="t('admin.search.embeddingEnabled')"
        :disable="savingSearch"
      />
      <div class="prts-dim q-mb-sm" style="font-size: 12px; margin-left: 52px">
        {{ t('admin.search.embeddingEnabledHint') }}
      </div>

      <!-- 密钥状态（只读徽标） -->
      <div class="row items-center q-mb-md" style="margin-left: 52px; gap: 8px; flex-wrap: wrap">
        <span style="font-size: 13px; color: var(--prts-text-dim)">{{ t('admin.search.keyPresent') }}：</span>
        <q-chip
          dense
          square
          :color="searchSettings.embedding_key_present ? 'positive' : 'grey-6'"
          text-color="white"
          :icon="searchSettings.embedding_key_present ? 'check_circle' : 'cancel'"
        >
          {{
            searchSettings.embedding_key_present
              ? t('admin.search.keyPresentYes')
              : t('admin.search.keyPresentNo')
          }}
        </q-chip>
        <span class="prts-dim" style="font-size: 11px">{{ t('admin.search.keyHint') }}</span>
      </div>

      <!-- 向量化启用但未配置密钥时的警告 -->
      <q-banner
        v-if="searchSettings.embedding_enabled && !searchSettings.embedding_key_present"
        dense
        rounded
        class="q-mb-md text-warning"
        style="background: var(--prts-bg-elev); border: 1px solid currentColor; font-size: 13px"
        icon="warning"
      >
        {{ t('admin.search.keyMissingWarning') }}
      </q-banner>

      <div class="row q-col-gutter-md q-mb-md">
        <div class="col-12 col-sm-6">
          <q-input
            v-model="searchSettings.embedding_model"
            outlined
            dense
            :label="t('admin.search.embeddingModel')"
            :disable="savingSearch"
          />
        </div>
        <div class="col-12 col-sm-6">
          <q-input
            v-model="searchSettings.embedding_base_url"
            outlined
            dense
            :label="t('admin.search.embeddingBaseUrl')"
            :disable="savingSearch"
          />
        </div>
        <div class="col-12 col-sm-4">
          <q-input
            v-model.number="searchSettings.embedding_batch"
            outlined
            dense
            type="number"
            :min="1"
            :max="10"
            :label="t('admin.search.embeddingBatch')"
            :disable="savingSearch"
          />
        </div>
      </div>

      <!-- TM 建议开关 -->
      <q-toggle
        v-model="searchSettings.tm_enabled"
        :label="t('admin.search.tmEnabled')"
        :disable="savingSearch"
      />
      <div class="prts-dim q-mb-sm" style="font-size: 12px; margin-left: 52px">
        {{ t('admin.search.tmEnabledHint') }}
      </div>

      <div class="row q-col-gutter-md q-mb-md">
        <div class="col-12 col-sm-4">
          <q-input
            v-model.number="searchSettings.tm_min_similarity"
            outlined
            dense
            type="number"
            :min="0"
            :max="1"
            :step="0.05"
            :label="t('admin.search.tmMinSimilarity')"
            :disable="savingSearch"
          />
        </div>
        <div class="col-12 col-sm-4">
          <q-input
            v-model.number="searchSettings.tm_top_n"
            outlined
            dense
            type="number"
            :min="1"
            :max="3"
            :label="t('admin.search.tmTopN')"
            :disable="savingSearch"
          />
        </div>
      </div>

      <div class="q-mt-sm">
        <q-btn
          unelevated
          no-caps
          color="primary"
          text-color="dark"
          :label="t('admin.search.save')"
          :loading="savingSearch"
          @click="saveSearchSettings"
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
