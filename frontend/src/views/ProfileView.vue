<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { useQuasar } from 'quasar'

import { apiErrorMessage, usersApi, type ApiKeyDto, type ExternalAccountDto } from '@/api'
import { COMMON_LANGS, langLabel } from '@/lib/langs'
import { roleLabel } from '@/lib/states'
import { useAuthStore } from '@/stores/auth'

const auth = useAuthStore()
const $q = useQuasar()

const desc = ref('')
const langs = ref<string[]>([])
const saving = ref(false)
const keys = ref<ApiKeyDto[]>([])
const accounts = ref<ExternalAccountDto[]>([])

onMounted(async () => {
  await auth.refreshMe()
  desc.value = auth.user?.description ?? ''
  langs.value = [...(auth.user?.translation_langs ?? [])]
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
    await usersApi.updateMe({ description: desc.value, translation_langs: langs.value })
    await auth.refreshMe()
    $q.notify({ type: 'positive', message: '资料已保存' })
  } catch (e) {
    $q.notify({ type: 'negative', message: apiErrorMessage(e, '保存失败') })
  } finally {
    saving.value = false
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
    $q.notify({ type: 'negative', message: apiErrorMessage(e, '创建失败') })
  }
}
function copyKey() {
  if (createdKey.value) {
    navigator.clipboard?.writeText(createdKey.value)
    $q.notify({ type: 'positive', message: '已复制', timeout: 800 })
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
    <h1 class="prts-h1 q-mb-lg">个人主页</h1>

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
              :label="roleLabel(auth.role)"
            />
          </div>
        </div>
        <q-space />
        <div class="text-center">
          <div class="prts-display text-accent" style="font-size: 28px">
            {{ Math.round(auth.user?.cp ?? 0) }}
          </div>
          <div class="prts-label">贡献分 CP</div>
        </div>
      </div>

      <q-separator class="q-my-lg" />

      <div class="column q-gutter-md">
        <div class="fld">
          <div class="prts-label q-mb-xs">个人描述</div>
          <q-input v-model="desc" outlined dense type="textarea" autogrow :disable="saving" />
        </div>
        <div class="fld">
          <div class="prts-label q-mb-xs">翻译语言偏好</div>
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
            :option-label="langLabel"
            :disable="saving"
          />
        </div>
        <div>
          <q-btn
            unelevated
            no-caps
            color="primary"
            text-color="dark"
            label="保存资料"
            :loading="saving"
            @click="saveProfile"
          />
        </div>
      </div>
    </q-card>

    <!-- linked accounts -->
    <div class="prts-label q-mb-sm">关联账号</div>
    <q-card flat bordered class="q-mb-lg">
      <q-list v-if="accounts.length" separator>
        <q-item v-for="a in accounts" :key="a.provider + a.external_id">
          <q-item-section avatar><q-icon name="link" color="primary" /></q-item-section>
          <q-item-section>
            <q-item-label>{{ a.provider }}</q-item-label>
            <q-item-label caption class="prts-mono">{{ a.external_id }}</q-item-label>
          </q-item-section>
        </q-item>
      </q-list>
      <div v-else class="prts-empty" style="padding: 30px">暂无关联账号</div>
    </q-card>

    <!-- api keys -->
    <div class="row items-center q-mb-sm">
      <div class="prts-label">API KEY</div>
      <q-space />
      <q-btn flat dense no-caps size="sm" icon="add" label="新建" @click="openCreate" />
    </div>
    <q-card flat bordered>
      <q-list v-if="keys.length" separator>
        <q-item v-for="k in keys" :key="k.id">
          <q-item-section>
            <q-item-label>{{ k.name }}</q-item-label>
            <q-item-label caption class="prts-mono"
              >{{ k.prefix }}··· · 创建于
              {{ new Date(k.created_at).toLocaleDateString() }}</q-item-label
            >
          </q-item-section>
          <q-item-section side>
            <q-btn
              flat
              round
              dense
              size="sm"
              icon="delete"
              color="negative"
              @click="revokeKey(k.id)"
            />
          </q-item-section>
        </q-item>
      </q-list>
      <div v-else class="prts-empty" style="padding: 30px">暂无 API Key</div>
    </q-card>

    <q-dialog v-model="showCreate">
      <q-card style="width: 440px; max-width: 92vw">
        <q-card-section><div class="prts-h2">新建 API Key</div></q-card-section>
        <q-card-section>
          <template v-if="!createdKey">
            <q-input
              v-model="newKeyName"
              outlined
              dense
              label="名称"
              autofocus
              @keyup.enter="createKey"
            />
          </template>
          <template v-else>
            <div class="prts-dim q-mb-sm" style="font-size: 13px">
              请立即复制，密钥仅显示这一次：
            </div>
            <q-input :model-value="createdKey" readonly outlined dense input-class="prts-mono">
              <template #append
                ><q-btn flat dense round icon="content_copy" @click="copyKey"
              /></template>
            </q-input>
          </template>
        </q-card-section>
        <q-card-actions align="right">
          <q-btn v-if="!createdKey" v-close-popup flat no-caps label="取消" />
          <q-btn
            v-if="!createdKey"
            unelevated
            no-caps
            color="primary"
            text-color="dark"
            label="创建"
            @click="createKey"
          />
          <q-btn v-else v-close-popup flat no-caps color="primary" label="完成" />
        </q-card-actions>
      </q-card>
    </q-dialog>
  </q-page>
</template>
