<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'

import { apiErrorMessage, projectsApi, type ProjectHistoryItemDto } from '@/api'
import { stateLabel } from '@/lib/states'
import { useProjectWorkspace } from '@/lib/projectWorkspace'

const { projectId } = useProjectWorkspace()
const { t } = useI18n()
const items = ref<ProjectHistoryItemDto[]>([])
const nextAfter = ref<string | null>(null)
const loading = ref(false)
const loadingMore = ref(false)
const error = ref('')

const hasItems = computed(() => items.value.length > 0)

async function load(reset = true) {
  if (reset) loading.value = true
  else loadingMore.value = true
  error.value = ''
  try {
    const page = await projectsApi.history(projectId.value, {
      after: reset ? undefined : (nextAfter.value ?? undefined),
      limit: 50,
    })
    items.value = reset ? page.items : [...items.value, ...page.items]
    nextAfter.value = page.next_after
  } catch (cause) {
    error.value = apiErrorMessage(cause, t('project.entryTimeline.loadFailed'))
  } finally {
    loading.value = false
    loadingMore.value = false
  }
}

function entryLink(item: ProjectHistoryItemDto) {
  return {
    name: 'editor',
    params: { id: projectId.value },
    query: { file: item.file_id, entry: item.entry_id },
  }
}

function formatValue(value: unknown) {
  if (value === null || value === undefined) return t('project.entryTimeline.emptyValue')
  if (typeof value === 'boolean') return value ? t('common.yes') : t('common.no')
  if (typeof value === 'object') return JSON.stringify(value)
  return String(value)
}

function changeLabel(field: ProjectHistoryItemDto['changes'][number]['field']) {
  return field === 'state'
    ? t('project.entryTimeline.fields.state')
    : t(`project.entryTimeline.fields.${field}`)
}

function stateValue(value: unknown) {
  return typeof value === 'string' ? stateLabel(value, t) : formatValue(value)
}

watch(projectId, () => void load(), { immediate: true })
</script>

<template>
  <section class="project-history">
    <div class="project-history__head">
      <div>
        <div class="prts-label">{{ t('project.sections.history') }}</div>
        <h2 class="prts-h2">{{ t('project.entryTimeline.heading') }}</h2>
      </div>
    </div>

    <q-banner v-if="error" dense class="bg-negative text-white">{{ error }}</q-banner>
    <q-inner-loading :showing="loading" />
    <div v-if="!loading && !hasItems" class="prts-empty">
      {{ t('project.entryTimeline.empty') }}
    </div>

    <article v-for="item in items" :key="item.id" class="project-history__item">
      <header class="project-history__item-head">
        <div class="project-history__resource">
          <q-icon name="mdi-file-document-outline" size="18px" />
          <div>
            <div class="prts-mono">{{ item.file_path }}</div>
            <router-link class="project-history__entry" :to="entryLink(item)">
              {{ item.entry_key }}
            </router-link>
          </div>
        </div>
        <div class="project-history__actor prts-dim">
          <q-avatar v-if="item.editor_avatar_url" size="24px">
            <img :src="item.editor_avatar_url" :alt="item.editor_name ?? ''" />
          </q-avatar>
          <span>{{ item.editor_name ?? t('editor.systemActor') }}</span>
          <time :datetime="item.created_at">{{ new Date(item.created_at).toLocaleString() }}</time>
        </div>
      </header>

      <div class="project-history__changes">
        <div v-for="change in item.changes" :key="change.field" class="project-history__change">
          <span class="prts-label">{{ changeLabel(change.field) }}</span>
          <span class="project-history__before">
            {{ change.field === 'state' ? stateValue(change.before) : formatValue(change.before) }}
          </span>
          <q-icon name="mdi-arrow-right" size="16px" />
          <span>
            {{ change.field === 'state' ? stateValue(change.after) : formatValue(change.after) }}
          </span>
        </div>
      </div>
    </article>

    <div v-if="nextAfter" class="row justify-center">
      <q-btn
        outline
        no-caps
        :loading="loadingMore"
        :label="t('project.entryTimeline.loadMore')"
        @click="load(false)"
      />
    </div>
  </section>
</template>

<style scoped>
.project-history {
  display: grid;
  gap: 14px;
}

.project-history__head,
.project-history__item-head,
.project-history__resource,
.project-history__actor,
.project-history__change {
  display: flex;
  align-items: center;
}

.project-history__item {
  display: grid;
  gap: 12px;
  padding: 14px;
  border: 1px solid var(--prts-border);
  background: var(--prts-panel);
}

.project-history__item-head {
  justify-content: space-between;
  gap: 16px;
}

.project-history__resource,
.project-history__actor {
  min-width: 0;
  gap: 8px;
}

.project-history__resource > div {
  min-width: 0;
}

.project-history__resource .prts-mono,
.project-history__entry {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.project-history__entry {
  display: block;
  color: var(--prts-accent);
}

.project-history__actor {
  flex: 0 0 auto;
  font-size: 12px;
}

.project-history__actor time {
  white-space: nowrap;
}

.project-history__changes {
  display: grid;
  gap: 6px;
}

.project-history__change {
  min-width: 0;
  gap: 7px;
  font-size: 13px;
}

.project-history__change > span:not(.prts-label) {
  min-width: 0;
  overflow-wrap: anywhere;
}

.project-history__before {
  color: var(--prts-text-dim);
}

@media (max-width: 599px) {
  .project-history__item-head {
    display: grid;
  }

  .project-history__actor {
    flex-wrap: wrap;
  }
}
</style>
