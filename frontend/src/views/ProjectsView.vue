<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { useQuasar } from 'quasar'

import { apiErrorMessage, projectsApi, type ProjectDto } from '@/api'
import { COMMON_LANGS, langLabel } from '@/lib/langs'
import { useAuthStore } from '@/stores/auth'

const auth = useAuthStore()
const router = useRouter()
const $q = useQuasar()

const tab = ref<'public' | 'mine'>('public')
const projects = ref<ProjectDto[]>([])
const loading = ref(false)

async function load() {
  loading.value = true
  try {
    projects.value = await projectsApi.list(tab.value === 'mine' ? { mine: true } : {})
  } catch (e) {
    $q.notify({ type: 'negative', message: apiErrorMessage(e) })
  } finally {
    loading.value = false
  }
}
watch(tab, load)
onMounted(load)

const showCreate = ref(false)
const creating = ref(false)
const form = ref({
  name: '',
  slug: '',
  description: '',
  source_langs: ['en'] as string[],
  primary_source_lang: 'en',
  target_lang: 'zh-Hans',
})
const isPrivate = ref(false)
const primaryOptions = computed(() => form.value.source_langs)

watch(
  () => form.value.source_langs,
  (languages) => {
    if (languages.length === 1 || !languages.includes(form.value.primary_source_lang)) {
      form.value.primary_source_lang = languages[0] ?? ''
    }
  },
  { deep: true },
)

async function create() {
  if (!form.value.name.trim() || !form.value.target_lang) return
  creating.value = true
  try {
    const p = await projectsApi.create({
      name: form.value.name.trim(),
      slug: form.value.slug.trim() || undefined,
      description: form.value.description.trim(),
      visibility: isPrivate.value ? 'private' : 'public',
      source_langs: form.value.source_langs,
      primary_source_lang: form.value.primary_source_lang,
      target_lang: form.value.target_lang,
    })
    showCreate.value = false
    $q.notify({ type: 'positive', message: '项目已创建' })
    router.push({ name: 'project-info', params: { id: p.id } })
  } catch (e) {
    $q.notify({ type: 'negative', message: apiErrorMessage(e, '创建失败') })
  } finally {
    creating.value = false
  }
}
</script>

<template>
  <q-page class="prts-container">
    <div class="row items-center q-mb-lg">
      <div>
        <div class="prts-label">// PROJECTS</div>
        <h1 class="prts-h1">项目</h1>
      </div>
      <q-space />
      <q-btn
        v-if="auth.canCreateProject"
        unelevated
        no-caps
        color="primary"
        text-color="dark"
        icon="mdi-plus"
        label="新建项目"
        @click="showCreate = true"
      />
    </div>

    <q-tabs
      v-model="tab"
      dense
      no-caps
      align="left"
      active-color="primary"
      indicator-color="primary"
      class="q-mb-md prts-dim"
      style="max-width: 280px"
    >
      <q-tab name="public" label="公开" />
      <q-tab v-if="auth.isAuthed" name="mine" label="我参与的" />
    </q-tabs>

    <div v-if="loading" class="row q-col-gutter-md">
      <div v-for="n in 6" :key="n" class="col-12 col-sm-6 col-md-4">
        <q-skeleton height="116px" square />
      </div>
    </div>
    <div v-else-if="projects.length === 0" class="prts-empty">暂无项目</div>
    <div v-else class="row q-col-gutter-md">
      <div v-for="p in projects" :key="p.id" class="col-12 col-sm-6 col-md-4">
        <q-card
          flat
          bordered
          class="proj-card cursor-pointer"
          @click="router.push({ name: 'project-info', params: { id: p.id } })"
        >
          <q-card-section>
            <div class="row items-center no-wrap">
              <div class="prts-h2 ellipsis">{{ p.name }}</div>
              <q-space />
              <q-badge v-if="p.visibility === 'private'" outline color="grey" label="私有" />
            </div>
            <div class="prts-mono prts-dim q-mt-xs" style="font-size: 11px">{{ p.slug }}</div>
            <div class="q-mt-sm row items-center prts-mono" style="font-size: 12px; gap: 6px">
              <span class="prts-dim">{{ p.source_langs.join(' · ') || '—' }}</span>
              <q-icon name="mdi-arrow-right" size="14px" class="prts-dim" />
              <span class="text-accent">{{ p.target_lang }}</span>
            </div>
            <div
              v-if="p.description"
              class="q-mt-sm prts-dim"
              style="font-size: 13px; line-height: 1.5; max-height: 40px; overflow: hidden"
            >
              {{ p.description }}
            </div>
          </q-card-section>
        </q-card>
      </div>
    </div>

    <q-dialog v-model="showCreate">
      <q-card style="width: 480px; max-width: 92vw">
        <q-card-section><div class="prts-h2">新建项目</div></q-card-section>
        <q-card-section class="column q-gutter-md">
          <q-input
            v-model="form.name"
            outlined
            dense
            label="项目名"
            :disable="creating"
            autofocus
          />
          <q-input
            v-model="form.slug"
            outlined
            dense
            label="Slug（可选，留空自动生成）"
            :disable="creating"
          />
          <q-input
            v-model="form.description"
            outlined
            dense
            type="textarea"
            autogrow
            label="描述（可选）"
            :disable="creating"
          />
          <q-select
            v-model="form.source_langs"
            outlined
            dense
            multiple
            use-chips
            use-input
            input-debounce="0"
            new-value-mode="add-unique"
            :options="COMMON_LANGS"
            :option-label="langLabel"
            label="源语言（可多选 / 自定义 BCP-47）"
            :disable="creating"
          />
          <q-select
            v-if="form.source_langs.length > 1"
            v-model="form.primary_source_lang"
            outlined
            dense
            emit-value
            :options="primaryOptions"
            :label="$t('project.primarySource')"
            :disable="creating"
          />
          <q-select
            v-model="form.target_lang"
            outlined
            dense
            use-input
            input-debounce="0"
            new-value-mode="add-unique"
            :options="COMMON_LANGS"
            :option-label="langLabel"
            label="目标语言（单一）"
            :disable="creating"
          />
          <q-toggle v-model="isPrivate" label="设为私有项目" :disable="creating" />
        </q-card-section>
        <q-card-actions align="right">
          <q-btn v-close-popup flat no-caps label="取消" :disable="creating" />
          <q-btn
            unelevated
            no-caps
            color="primary"
            text-color="dark"
            label="创建"
            :loading="creating"
            @click="create"
          />
        </q-card-actions>
      </q-card>
    </q-dialog>
  </q-page>
</template>

<style scoped>
.proj-card {
  transition:
    border-color 0.15s ease,
    transform 0.15s ease;
}
.proj-card:hover {
  border-color: var(--prts-accent);
  transform: translateY(-2px);
}
</style>
