<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useQuasar } from 'quasar'
import { useI18n } from 'vue-i18n'

import { adminApi, adminSearchApi, apiErrorMessage, posApi } from '@/api'
import type { PosDto, PosWriteRequest, SearchSettingsDto } from '@/api'
import TermImportDialog from '@/components/terms/TermImportDialog.vue'
import { displayPosName, type TerminologyDocumentFormat } from '@/lib/terminology'
import { useAuthStore } from '@/stores/auth'

const auth = useAuthStore()
const $q = useQuasar()
const { t, locale } = useI18n()

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
    $q.notify({ type: 'positive', message: t('admin.settingsSaved') })
  } catch (e) {
    $q.notify({ type: 'negative', message: apiErrorMessage(e, t('admin.settingsSaveFailed')) })
  } finally {
    savingSettings.value = false
  }
}

/* —— 角色任免（仅总管理员）—— */
const grantUserId = ref<number | null>(null)
const grantRoleVal = ref<string | null>('maintainer')
const granting = ref(false)
const roleOptions = computed(() => [
  { label: t('admin.roles.superAdmin'), value: 'super_admin' },
  { label: t('admin.roles.admin'), value: 'admin' },
  { label: t('admin.roles.maintainer'), value: 'maintainer' },
  { label: t('admin.roles.ordinary'), value: null },
])
async function doGrant() {
  if (grantUserId.value == null) return
  granting.value = true
  try {
    await adminApi.grantRole(grantUserId.value, grantRoleVal.value)
    $q.notify({ type: 'positive', message: t('admin.roles.updated') })
    grantUserId.value = null
  } catch (e) {
    $q.notify({ type: 'negative', message: apiErrorMessage(e, t('admin.roles.updateFailed')) })
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

/* —— 双语 POS 预设 —— */
const posPresets = ref<PosDto[]>([])
const posLoading = ref(false)
const posSaving = ref(false)
const posDialogOpen = ref(false)
const posImportOpen = ref(false)
const editingPosId = ref<number | null>(null)
const posForm = ref<PosWriteRequest>({ name_zh_cn: null, name_en: null, sort_order: 0 })

async function loadPosPresets() {
  if (!auth.canManagePos) return
  posLoading.value = true
  try {
    posPresets.value = await posApi.list()
  } catch (error) {
    $q.notify({ type: 'negative', message: apiErrorMessage(error, t('terminology.pos.loadFailed')) })
  } finally {
    posLoading.value = false
  }
}
onMounted(loadPosPresets)

function openCreatePos() {
  editingPosId.value = null
  posForm.value = { name_zh_cn: null, name_en: null, sort_order: 0 }
  posDialogOpen.value = true
}

function openEditPos(pos: PosDto) {
  editingPosId.value = pos.id
  posForm.value = {
    name_zh_cn: pos.name_zh_cn,
    name_en: pos.name_en,
    sort_order: pos.sort_order,
  }
  posDialogOpen.value = true
}

async function savePos() {
  posSaving.value = true
  try {
    if (editingPosId.value == null) {
      await posApi.create(posForm.value)
    } else {
      await posApi.update(editingPosId.value, posForm.value)
    }
    $q.notify({ type: 'positive', message: t('terminology.pos.saved') })
    posDialogOpen.value = false
    await loadPosPresets()
  } catch (error) {
    $q.notify({ type: 'negative', message: apiErrorMessage(error, t('terminology.pos.saveFailed')) })
  } finally {
    posSaving.value = false
  }
}

function confirmDeletePos(pos: PosDto) {
  $q.dialog({
    title: t('terminology.pos.delete'),
    message: t('terminology.pos.deleteConfirm'),
    cancel: true,
    persistent: true,
  }).onOk(async () => {
    try {
      await posApi.remove(pos.id)
      $q.notify({ type: 'positive', message: t('terminology.pos.deleted') })
      await loadPosPresets()
    } catch (error) {
      $q.notify({
        type: 'negative',
        message: apiErrorMessage(error, t('terminology.pos.deleteFailed')),
      })
    }
  })
}

async function exportPos(format: TerminologyDocumentFormat) {
  try {
    const blob = await posApi.export(format)
    const url = URL.createObjectURL(blob)
    const anchor = document.createElement('a')
    anchor.href = url
    anchor.download = `pos-presets.${format}`
    anchor.click()
    URL.revokeObjectURL(url)
  } catch (error) {
    $q.notify({ type: 'negative', message: apiErrorMessage(error, t('terminology.exportFailed')) })
  }
}
</script>

<template>
  <q-page class="prts-container prts-container--narrow">
    <div class="prts-label">// ADMIN</div>
    <h1 class="prts-h1 q-mb-lg">{{ t('admin.title') }}</h1>

    <div class="prts-label q-mb-sm">{{ t('admin.platformSettings') }}</div>
    <q-card flat bordered class="q-pa-md q-mb-lg">
      <q-toggle
        v-model="registrationOpen"
        :label="t('admin.registrationOpen')"
        :disable="savingSettings"
      />
      <div class="prts-dim q-mb-md" style="font-size: 12px; margin-left: 52px">
        {{ t('admin.registrationOpenHint') }}
      </div>
      <q-toggle
        v-model="oauthOnly"
        :label="t('admin.oauthOnly')"
        :disable="savingSettings"
      />
      <div class="prts-dim q-mb-md" style="font-size: 12px; margin-left: 52px">
        {{ t('admin.oauthOnlyHint') }}
      </div>
      <q-toggle
        v-model="requireEmail"
        :label="t('admin.requireEmail')"
        :disable="savingSettings"
      />
      <div class="prts-dim" style="font-size: 12px; margin-left: 52px">
        {{ t('admin.requireEmailHint') }}
      </div>
      <div class="q-mt-md">
        <q-btn
          unelevated
          no-caps
          color="primary"
          text-color="dark"
          :label="t('admin.saveSettings')"
          :loading="savingSettings"
          @click="saveSettings"
        />
      </div>
    </q-card>

    <div v-if="auth.canManagePos" class="prts-label q-mb-sm">
      {{ t('terminology.pos.heading') }}
    </div>
    <q-card v-if="auth.canManagePos" flat bordered class="q-pa-md q-mb-lg">
      <div class="row items-start justify-between q-gutter-md q-mb-md">
        <div class="prts-dim" style="font-size: 13px">
          {{ t('terminology.pos.description') }}
        </div>
        <div class="row q-gutter-sm">
          <q-btn-dropdown outline no-caps icon="mdi-download-outline" :label="t('terminology.export')">
            <q-list>
              <q-item v-close-popup clickable @click="exportPos('csv')">
                <q-item-section>CSV</q-item-section>
              </q-item>
              <q-item v-close-popup clickable @click="exportPos('json')">
                <q-item-section>JSON</q-item-section>
              </q-item>
            </q-list>
          </q-btn-dropdown>
          <q-btn
            outline
            no-caps
            icon="mdi-file-import-outline"
            :label="t('terminology.import.action')"
            @click="posImportOpen = true"
          />
          <q-btn
            unelevated
            no-caps
            color="primary"
            text-color="dark"
            icon="mdi-plus"
            :label="t('terminology.pos.create')"
            @click="openCreatePos"
          />
        </div>
      </div>

      <q-inner-loading :showing="posLoading" />
      <div v-if="!posLoading && posPresets.length === 0" class="prts-empty">
        {{ t('terminology.pos.empty') }}
      </div>
      <q-markup-table v-else flat bordered separator="horizontal">
        <thead>
          <tr>
            <th>{{ t('terminology.pos.displayName') }}</th>
            <th>{{ t('terminology.pos.nameZh') }}</th>
            <th>{{ t('terminology.pos.nameEn') }}</th>
            <th>{{ t('terminology.pos.sortOrder') }}</th>
            <th class="text-right">{{ t('terminology.fields.actions') }}</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="pos in posPresets" :key="pos.id">
            <td>{{ displayPosName(pos, locale) || `#${pos.id}` }}</td>
            <td>{{ pos.name_zh_cn || '—' }}</td>
            <td>{{ pos.name_en || '—' }}</td>
            <td>{{ pos.sort_order }}</td>
            <td class="text-right">
              <q-btn flat round dense icon="mdi-pencil-outline" @click="openEditPos(pos)" />
              <q-btn
                flat
                round
                dense
                color="negative"
                icon="mdi-delete-outline"
                @click="confirmDeletePos(pos)"
              />
            </td>
          </tr>
        </tbody>
      </q-markup-table>
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
        icon="mdi-alert-outline"
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
      <div class="prts-label q-mb-sm">{{ t('admin.roles.title') }}</div>
      <q-card flat bordered class="q-pa-md">
        <div class="prts-dim q-mb-md" style="font-size: 13px">
          {{ t('admin.roles.description') }}
        </div>
        <div class="row q-col-gutter-md items-center">
          <div class="col-12 col-sm-4">
            <q-input
              v-model.number="grantUserId"
              outlined
              dense
              type="number"
              :label="t('admin.roles.userId')"
            />
          </div>
          <div class="col-12 col-sm-5">
            <q-select
              v-model="grantRoleVal"
              outlined
              dense
              :options="roleOptions"
              emit-value
              map-options
              :label="t('admin.roles.role')"
            />
          </div>
          <div class="col-12 col-sm-3">
            <q-btn
              unelevated
              no-caps
              color="primary"
              text-color="dark"
              class="full-width"
              :label="t('admin.roles.apply')"
              :loading="granting"
              @click="doGrant"
            />
          </div>
        </div>
      </q-card>
    </template>

    <q-dialog v-if="auth.canManagePos" v-model="posDialogOpen" persistent>
      <q-card style="width: min(560px, 94vw)">
        <q-card-section>
          <div class="prts-label">POS</div>
          <div class="prts-h2">
            {{ editingPosId == null ? t('terminology.pos.create') : t('terminology.pos.edit') }}
          </div>
        </q-card-section>
        <q-card-section class="q-gutter-md">
          <q-input
            v-model="posForm.name_zh_cn"
            outlined
            dense
            clearable
            :label="t('terminology.pos.nameZh')"
          />
          <q-input
            v-model="posForm.name_en"
            outlined
            dense
            clearable
            :label="t('terminology.pos.nameEn')"
          />
          <q-input
            v-model.number="posForm.sort_order"
            outlined
            dense
            type="number"
            :label="t('terminology.pos.sortOrder')"
          />
          <div class="prts-dim" style="font-size: 12px">
            {{ t('terminology.pos.nameRequired') }}
          </div>
        </q-card-section>
        <q-card-actions align="right">
          <q-btn v-close-popup flat no-caps :label="t('project.cancel')" />
          <q-btn
            unelevated
            no-caps
            color="primary"
            text-color="dark"
            :label="t('project.save')"
            :loading="posSaving"
            :disable="!posForm.name_zh_cn?.trim() && !posForm.name_en?.trim()"
            @click="savePos"
          />
        </q-card-actions>
      </q-card>
    </q-dialog>

    <TermImportDialog
      v-if="auth.canManagePos"
      v-model="posImportOpen"
      kind="pos"
      @confirmed="loadPosPresets"
    />
  </q-page>
</template>
