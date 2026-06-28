<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useRoute } from 'vue-router'
import { useQuasar } from 'quasar'

import {
  apiErrorMessage,
  entriesApi,
  projectsApi,
  type EntryDto,
  type EntryVersionDto,
  type FileDto,
  type MemberDto,
  type ProjectDto,
} from '@/api'
import { STATE_LABELS, STATE_ORDER, stateLabel } from '@/lib/states'
import { useAuthStore } from '@/stores/auth'

const props = defineProps<{ id: number }>()
const route = useRoute()
const auth = useAuthStore()
const $q = useQuasar()

const project = ref<ProjectDto | null>(null)
const files = ref<FileDto[]>([])
const members = ref<MemberDto[]>([])

/* —— 权限 —— */
const myRole = computed(() => {
  if (auth.isAdmin) return 'owner'
  return members.value.find((m) => m.user_id === auth.user?.id)?.role ?? null
})
const isMember = computed(() => myRole.value !== null)
const canReview = computed(() => ['owner', 'manager', 'reviewer'].includes(myRole.value ?? ''))
const canEditLocked = computed(() => ['owner', 'manager'].includes(myRole.value ?? ''))
const canFlag = computed(() => ['owner', 'manager'].includes(myRole.value ?? ''))
const availableStates = computed(() =>
  canReview.value ? STATE_ORDER : ['untranslated', 'translated', 'questioned'],
)

/* —— 筛选 —— */
const currentFileId = ref<number | null>(route.query.file ? Number(route.query.file) : null)
const search = ref('')
const stateFilter = ref<string[]>([])
const includeHidden = ref(false)
const fileOptions = computed(() => [
  { label: '全部文件', value: null as number | null },
  ...files.value.map((f) => ({ label: f.path, value: f.id })),
])

/* —— 列表（键集分页累加）—— */
const entries = ref<EntryDto[]>([])
const listLoading = ref(false)
const hasMore = ref(true)
const PAGE = 80

async function resetAndLoad() {
  entries.value = []
  hasMore.value = true
  await loadMore()
}
async function loadMore() {
  if (listLoading.value || !hasMore.value) return
  listLoading.value = true
  try {
    const after = entries.value.length ? entries.value[entries.value.length - 1].id : undefined
    const batch = await entriesApi.list(props.id, {
      file_id: currentFileId.value ?? undefined,
      state: stateFilter.value.length ? stateFilter.value.join(',') : undefined,
      q: search.value.trim() || undefined,
      after,
      limit: PAGE,
      include_hidden: includeHidden.value,
    })
    entries.value.push(...batch)
    hasMore.value = batch.length === PAGE
  } catch (e) {
    $q.notify({ type: 'negative', message: apiErrorMessage(e) })
  } finally {
    listLoading.value = false
  }
}

let searchTimer: ReturnType<typeof setTimeout> | undefined
watch(search, () => {
  clearTimeout(searchTimer)
  searchTimer = setTimeout(resetAndLoad, 300)
})
watch([currentFileId, stateFilter, includeHidden], resetAndLoad, { deep: true })

interface VScrollDetails {
  to: number
}
function onScroll(details: VScrollDetails) {
  if (details.to >= entries.value.length - 12) loadMore()
}

/* —— 选中与编辑 —— */
const selected = ref<EntryDto | null>(null)
const draft = ref('')
const draftState = ref('untranslated')
const saving = ref(false)

const panelReadOnly = computed(
  () => !isMember.value || (selected.value?.locked === true && !canEditLocked.value),
)

function select(e: EntryDto) {
  selected.value = e
  draft.value = e.translation
  draftState.value = e.state
}

const sourceLangs = computed(() => project.value?.source_langs ?? [])

async function save() {
  if (!selected.value) return
  saving.value = true
  try {
    const updated = await entriesApi.update(props.id, selected.value.id, {
      translation: draft.value,
      state: draftState.value,
      version: selected.value.version,
    })
    applyUpdated(updated)
    $q.notify({ type: 'positive', message: '已保存', timeout: 900 })
    selectNext()
  } catch (e) {
    const err = e as { response?: { status?: number } }
    if (err.response?.status === 409) {
      $q.notify({ type: 'warning', message: '该词条已被他人修改，已刷新为最新' })
      const fresh = await entriesApi.get(props.id, selected.value.id)
      applyUpdated(fresh)
      select(fresh)
    } else {
      $q.notify({ type: 'negative', message: apiErrorMessage(e, '保存失败') })
    }
  } finally {
    saving.value = false
  }
}

function applyUpdated(u: EntryDto) {
  const idx = entries.value.findIndex((x) => x.id === u.id)
  if (idx >= 0) entries.value[idx] = u
  if (selected.value?.id === u.id) selected.value = u
}

function selectNext() {
  if (!selected.value) return
  const idx = entries.value.findIndex((x) => x.id === selected.value!.id)
  if (idx >= 0 && idx + 1 < entries.value.length) select(entries.value[idx + 1])
}

async function toggleFlag(flag: 'locked' | 'hidden') {
  if (!selected.value) return
  try {
    const body =
      flag === 'locked' ? { locked: !selected.value.locked } : { hidden: !selected.value.hidden }
    const updated = await entriesApi.setFlags(props.id, selected.value.id, body)
    applyUpdated(updated)
  } catch (e) {
    $q.notify({ type: 'negative', message: apiErrorMessage(e, '操作失败') })
  }
}

/* —— 历史 —— */
const showHistory = ref(false)
const history = ref<EntryVersionDto[]>([])
async function openHistory() {
  if (!selected.value) return
  try {
    history.value = await entriesApi.history(props.id, selected.value.id)
    showHistory.value = true
  } catch (e) {
    $q.notify({ type: 'negative', message: apiErrorMessage(e) })
  }
}

const split = ref(34)

onMounted(async () => {
  try {
    const [p, tree, mem] = await Promise.all([
      projectsApi.get(props.id),
      projectsApi.tree(props.id),
      projectsApi.members(props.id),
    ])
    project.value = p.project
    files.value = tree.files
    members.value = mem
  } catch (e) {
    $q.notify({ type: 'negative', message: apiErrorMessage(e, '加载失败') })
  }
  await resetAndLoad()
})
</script>

<template>
  <q-page class="editor-page">
    <!-- toolbar -->
    <div class="editor-bar">
      <q-btn flat dense round icon="arrow_back" :to="{ name: 'project', params: { id: props.id } }">
        <q-tooltip>返回项目</q-tooltip>
      </q-btn>
      <div class="prts-display ellipsis" style="font-size: 14px; max-width: 220px">
        {{ project?.name ?? '…' }}
      </div>
      <q-select
        v-model="currentFileId"
        :options="fileOptions"
        dense
        outlined
        options-dense
        emit-value
        map-options
        style="min-width: 180px; max-width: 280px"
      />
      <q-input
        v-model="search"
        dense
        outlined
        clearable
        debounce="0"
        placeholder="搜索 key / 原文 / 译文"
        style="min-width: 200px; flex: 1"
      >
        <template #prepend><q-icon name="search" /></template>
      </q-input>
      <q-select
        v-model="stateFilter"
        :options="STATE_ORDER"
        :option-label="(s) => STATE_LABELS[s] ?? s"
        dense
        outlined
        multiple
        options-dense
        emit-value
        map-options
        placeholder="状态"
        style="min-width: 150px"
      />
      <q-toggle v-if="isMember" v-model="includeHidden" label="含隐藏" dense />
    </div>

    <q-splitter v-model="split" :limits="[26, 58]" class="editor-split">
      <!-- list -->
      <template #before>
        <q-virtual-scroll :items="entries" class="entry-list" @virtual-scroll="onScroll">
          <template #default="{ item }">
            <div
              class="entry-row"
              :class="{ active: item.id === selected?.id }"
              @click="select(item)"
            >
              <span class="state-dot" :class="'state-' + item.state" />
              <div class="entry-row__body">
                <div class="entry-row__key prts-mono">{{ item.key }}</div>
                <div class="entry-row__preview">
                  {{ item.translation || Object.values(item.original)[0] || '—' }}
                </div>
              </div>
              <q-icon v-if="item.locked" name="lock" size="14px" class="prts-dim" />
              <q-icon v-if="item.hidden" name="visibility_off" size="14px" class="prts-dim" />
            </div>
          </template>
        </q-virtual-scroll>
        <div v-if="listLoading" class="row justify-center q-pa-sm">
          <q-spinner color="primary" size="20px" />
        </div>
        <div v-else-if="entries.length === 0" class="prts-empty">无匹配词条</div>
      </template>

      <!-- panel -->
      <template #after>
        <div v-if="!selected" class="prts-empty" style="padding-top: 120px">
          从左侧选择一个词条开始翻译
        </div>
        <div v-else class="panel">
          <div class="row items-center q-mb-sm">
            <div class="prts-label">KEY</div>
            <div class="prts-mono q-ml-sm" style="font-size: 13px">{{ selected.key }}</div>
            <q-space />
            <q-btn flat dense round size="sm" icon="history" @click="openHistory">
              <q-tooltip>历史</q-tooltip>
            </q-btn>
            <q-btn
              v-if="canFlag"
              flat
              dense
              round
              size="sm"
              :icon="selected.locked ? 'lock' : 'lock_open'"
              :color="selected.locked ? 'amber' : undefined"
              @click="toggleFlag('locked')"
            >
              <q-tooltip>{{ selected.locked ? '解锁' : '锁定' }}</q-tooltip>
            </q-btn>
            <q-btn
              v-if="canFlag"
              flat
              dense
              round
              size="sm"
              :icon="selected.hidden ? 'visibility_off' : 'visibility'"
              @click="toggleFlag('hidden')"
            >
              <q-tooltip>{{ selected.hidden ? '取消隐藏' : '隐藏' }}</q-tooltip>
            </q-btn>
          </div>

          <!-- original per source lang -->
          <div class="orig-block">
            <div v-for="lang in sourceLangs" :key="lang" class="orig-row">
              <div class="prts-label orig-lang">{{ lang }}</div>
              <div class="orig-text">{{ selected.original[lang] ?? '—' }}</div>
            </div>
            <div v-if="selected.context" class="orig-row">
              <div class="prts-label orig-lang">注释</div>
              <div class="orig-text prts-dim">{{ selected.context }}</div>
            </div>
          </div>

          <div class="prts-label q-mt-md q-mb-xs">译文 → {{ project?.target_lang }}</div>
          <q-input
            v-model="draft"
            type="textarea"
            outlined
            autogrow
            :readonly="panelReadOnly"
            input-class="prts-translation"
            :input-style="{ minHeight: '120px' }"
          />

          <div class="row items-center q-mt-md q-gutter-sm">
            <q-select
              v-model="draftState"
              :options="availableStates"
              :option-label="(s) => STATE_LABELS[s] ?? s"
              dense
              outlined
              emit-value
              map-options
              :disable="panelReadOnly"
              style="min-width: 140px"
            />
            <q-space />
            <span v-if="panelReadOnly" class="prts-dim prts-mono" style="font-size: 12px">
              {{ selected.locked ? '已锁定 · 只读' : '无编辑权限' }}
            </span>
            <q-btn
              unelevated
              no-caps
              color="primary"
              text-color="dark"
              icon="save"
              label="保存"
              :loading="saving"
              :disable="panelReadOnly"
              @click="save"
            />
          </div>
        </div>
      </template>
    </q-splitter>

    <!-- history dialog -->
    <q-dialog v-model="showHistory">
      <q-card style="width: 560px; max-width: 94vw">
        <q-card-section><div class="prts-h2">词条历史</div></q-card-section>
        <q-card-section style="max-height: 60vh; overflow: auto">
          <q-timeline v-if="history.length" color="primary">
            <q-timeline-entry
              v-for="(h, i) in history"
              :key="i"
              :subtitle="new Date(h.created_at).toLocaleString()"
            >
              <template #title>
                <span class="prts-mono" style="font-size: 13px"
                  >v{{ h.version }} · {{ h.kind }}</span
                >
                <q-badge v-if="h.state" outline class="q-ml-sm" :label="stateLabel(h.state)" />
              </template>
              <div v-if="h.translation" class="prts-translation">{{ h.translation }}</div>
            </q-timeline-entry>
          </q-timeline>
          <div v-else class="prts-empty">暂无历史</div>
        </q-card-section>
        <q-card-actions align="right"
          ><q-btn v-close-popup flat no-caps label="关闭"
        /></q-card-actions>
      </q-card>
    </q-dialog>
  </q-page>
</template>

<style scoped>
.editor-page {
  height: calc(100vh - var(--prts-nav-h));
  display: flex;
  flex-direction: column;
}
.editor-bar {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 12px;
  border-bottom: 1px solid var(--prts-border);
  background: var(--prts-panel);
}
.editor-split {
  flex: 1;
  min-height: 0;
}
.entry-list {
  height: 100%;
}
.entry-row {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 14px;
  border-bottom: 1px solid var(--prts-border-soft);
  cursor: pointer;
}
.entry-row:hover {
  background: var(--prts-panel-2);
}
.entry-row.active {
  background: var(--prts-accent-dim);
  box-shadow: inset 2px 0 0 var(--prts-accent);
}
.entry-row__body {
  min-width: 0;
  flex: 1;
}
.entry-row__key {
  font-size: 11px;
  color: var(--prts-text-dim);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.entry-row__preview {
  font-size: 13px;
  color: var(--prts-text);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.panel {
  padding: 22px 26px;
  height: 100%;
  overflow: auto;
}
.orig-block {
  border: 1px solid var(--prts-border);
  border-radius: var(--prts-radius);
  background: var(--prts-bg-elev);
  overflow: hidden;
}
.orig-row {
  display: flex;
  gap: 14px;
  padding: 11px 14px;
  border-bottom: 1px solid var(--prts-border-soft);
}
.orig-row:last-child {
  border-bottom: none;
}
.orig-lang {
  flex: 0 0 56px;
  padding-top: 2px;
}
.orig-text {
  font-size: 14px;
  line-height: 1.6;
  white-space: pre-wrap;
  color: var(--prts-text-strong);
}
:deep(.prts-translation) {
  font-size: 14px;
  line-height: 1.7;
}
</style>
